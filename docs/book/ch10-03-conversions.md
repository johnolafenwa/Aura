# Converting between types

In Python, `2 + 0.5` is `2.5`, `len(items) / 2` is a float, and an integer
quietly grows as large as it needs to be. Aura does none of these things. A
value keeps its type until you write a conversion, and every conversion you
write either produces an exact result, traps, or returns a `Result`. This
section shows the three tools for numbers and text, explains why Aura is this
strict, and ends with conversions between your own types.

## No implicit conversions

The error that surprises most newcomers is a good place to begin. A sensor
gives us an `int32`, and a helper takes an `int64`:

```aura check-fail:AU2002
def scale(value: int64) -> int64:
    return value * 2

def main():
    reading: int32 = 21
    print(scale(reading))
```

```console
$ aura check src/main.au
error[AU2002]: argument type mismatch for function `scale`: expected `int64`, found `int32`
 --> src/main.au:6:17
  |
6 |     print(scale(reading))
  |                 ^
```

Every `int32` fits in an `int64`, so this looks like a conversion the compiler
could do for us. Aura won't, and the reason is that silent widening never comes
alone. A language that widens `int32` to `int64` on the way into a function
soon needs rules for which way to convert when an `int32` meets a `uint32`, or
an `int64` meets a `float64`, and somewhere in those rules a value gets
narrowed or rounded without anyone writing it. Overflow and precision bugs live
in exactly those places. Aura's answer is that the code shows every conversion,
so you can find them.

The same rule applies to operators. Both sides of `+` must have the same type:

```aura check-fail:AU2999
def main():
    a: int32 = 2
    b: int64 = 3
    print(a + b)
```

```console
$ aura check src/main.au
error[AU2999]: binary operator operands must match, found `int32` and `int64`
 --> src/main.au:4:11
  |
4 |     print(a + b)
  |           ^
```

In practice this comes up less often than you might fear. An integer literal
with no other type information is an `int64`, a float literal is a `float64`,
and lengths and indices are `int64`, so most code uses one integer type and one
float type throughout. Mixed widths appear at the edges: a file format with
32-bit fields, a C library, a protocol. There is one deliberate exception. An
index position, such as `names[i]`, accepts smaller integer types like `int32`
and widens them, because widening an index can't lose information and indexing
with a narrow counter is common.

## `as` for numbers

To convert between numeric types, write `value as Type`:

```aura
def scale(value: int64) -> int64:
    return value * 2

def main():
    reading: int32 = 21
    print(scale(reading as int64))
```

`as` works between any two numeric types, and it follows one rule: the result
is exact, or the program traps. Widening is always exact. Narrowing is exact
when the value fits, and traps when it doesn't:

```aura
def main():
    small: int32 = 7
    print(small as int64)
    big: int64 = 300
    print(big as int16)
    print(3 as float64)
    print(3.9 as int64)
    print(-3.9 as int64)
```

```console
$ aura run src/main.au
7
300
3.0
3
-3
```

Converting a float to an integer truncates toward zero, so `3.9` becomes `3`
and `-3.9` becomes `-3`. That's the only kind of loss `as` allows, and it's the
one you'd expect from the operation's name. Now let's narrow a value that
doesn't fit:

```aura
def main():
    count: int64 = 300
    as_byte = count as uint8
    print(as_byte)
```

```console
$ aura run src/main.au
error[AU4002]: integer value `300` does not fit in `uint8`
 --> src/main.au:3:15
  |
3 |     as_byte = count as uint8
  |               ^
  = note: Aura call chain (innermost first): main at 1:1
```

In C, `(uint8_t)300` is `44`, and the program carries on with a number nobody
asked for. Aura traps with the same `AU4002` you saw for arithmetic overflow in
[Chapter 9](/book/ch09-01-traps). When the value is a literal, the compiler
doesn't wait for run time: `300 as uint8` is rejected by `aura check` with
`integer literal `300` does not fit in `uint8``.

Integer to float conversion follows the same rule, which is less obvious. A
`float64` has 53 bits of precision, so not every `int64` has an exact
`float64`:

```aura
def main():
    n: int64 = 9007199254740993
    f = n as float64
    print(f)
```

```console
$ aura run src/main.au
error[AU4002]: integer value `9007199254740993` cannot be represented exactly as `float64`
 --> src/main.au:3:9
  |
3 |     f = n as float64
  |         ^
  = note: Aura call chain (innermost first): main at 1:1
```

The nearest `float64` is `9007199254740992`, one less. `as` won't give you that
silently. A float that isn't a number at all can't become an integer either:
casting infinity to `int64` traps with `cannot cast non-finite float to
`int64``.

Read `as` as a claim you're making: "this value fits the target type". If the
claim is false, you find out at the line that made it.

## `.to_float()` when rounding is the point

Sometimes you want the nearest float and don't care about the last bit.
Computing an average is the usual case. Every integer type has a `.to_float()`
method that returns a `float64`, rounding to the nearest representable value
instead of trapping:

```aura
def average(prices: list[int64]) -> float64:
    mut total = 0
    for price in prices:
        total += price
    return total.to_float() / prices.len().to_float()

def main():
    print(average([1300, 450, 1700]))
    n: int64 = 9007199254740993
    print(n.to_float())
```

```console
$ aura run src/main.au
1150.0
9007199254740992.0
```

`average` shows the other reason you'll meet `.to_float()` often. Aura doesn't
define `/` on two integers:

```aura check-fail:AU2003
def average(total: int64, count: int64) -> float64:
    return total / count
```

```console
$ aura check src/main.au
error[AU2003]: integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
 --> src/main.au:2:12
  |
2 |     return total / count
  |            ^
```

Python 2 had `7 / 2 == 3`, Python 3 changed it to `3.5`, and code broke in both
directions. Aura makes you say which one you mean. `//` is floor division and
stays an integer; `.to_float()` on both sides gives true division. The help
line in the diagnostic says exactly that.

Choose between `as float64` and `.to_float()` by intent. `as float64` says the
value must be exact. `.to_float()` says rounding is acceptable.

## Text and numbers

`as` refuses anything that isn't numeric:

```aura check-fail:AU2002
def main():
    text = "12"
    n = text as int64
```

```console
$ aura check src/main.au
error[AU2002]: casts are only supported between numeric types, found `str` and `int64`
 --> src/main.au:3:9
  |
3 |     n = text as int64
  |         ^
```

Converting text to a number can fail for reasons that have nothing to do with
bugs, so it isn't a cast: it's parsing, and parsing returns a `Result`. You
used `parse_int64` throughout Chapter 9; `parse_int32` and `parse_float64` work
the same way.

Going the other direction always succeeds, so it returns a plain `str`. Use
`str(value)`, or put the value in an f-string, which also lets you format it:

```aura
def main():
    count: int64 = 42
    print(str(count) + " items")
    ratio = 1300.to_float() / 3.to_float()
    print(f"{ratio:.2f}")
```

For your own types, `str()` and f-strings use `Display`, which you implemented
for `Money` in the previous section.

## Converting your own types

There's no `as` for classes. To turn one of your types into another, or a
builtin value into one of yours, you write a function or a method, and two
traits give that code a conventional shape.

The first is `From`, which you met in [Chapter
9](/book/ch09-02-recoverable-errors) as the way `try` converts one error type
into another. Nothing about it is specific to errors. `From[Source]` has a
single method, `from`, with no `self`: it takes a `Source` and returns a value
of the implementing type. A method with no `self` is called an *associated
method*, and you call it on the type, as in `Money.from(12)`.

The second is a trait we'll define ourselves for parsing text into a value. Its
method also has no `self`, and returns `Result[Self, str]`, where `Self` means
"the type implementing this trait". Here are both, implemented for `Money`:

*Filename: src/main.au*

```aura
trait From[Source]:
    def from(value: own Source) -> Self

trait Parse:
    def parse(text: str) -> Result[Self, str]

copy class Money:
    cents: int64

impl Display for Money:
    def display(self) -> str:
        return f"${self.cents // 100}.{self.cents % 100:02}"

impl From[int64] for Money:
    def from(value: own int64) -> Money:
        return Money(cents=value * 100)

impl Parse for Money:
    def parse(text: str) -> Result[Money, str]:
        if text.starts_with("-"):
            return Result.Err(f"`{text}`: a price cannot be negative")
        parts = text.split(".")
        if parts.len() == 1:
            dollars = try parse_int64(parts[0])
            return Result.Ok(Money.from(dollars))
        if parts.len() != 2 or parts[1].len() != 2:
            return Result.Err(f"`{text}` is not a price like 12.50")
        dollars = try parse_int64(parts[0])
        cents = try parse_int64(parts[1])
        return Result.Ok(Money(cents=dollars * 100 + cents))

def parse_all[T: Parse](texts: list[str]) -> Result[list[T], str]:
    mut values: list[T] = []
    for text in texts:
        values.append(try T.parse(text))
    return Result.Ok(values)

def average_dollars(prices: list[Money]) -> float64:
    mut total = 0
    for price in prices:
        total += price.cents
    return total.to_float() / prices.len().to_float() / 100.0

def main():
    print(Money.from(12))
    print(Money.parse("4.05"))
    print(Money.parse("4.5"))
    print(Money.parse("-3"))
    match parse_all[Money](["12.50", "3", "0.99"]):
        case Result.Ok(prices):
            print(prices)
            print(f"average: {average_dollars(prices):.2f}")
        case Result.Err(message):
            print(message)
```

<span class="caption">Listing 10-9: Converting integers and text into `Money` with `From` and a `Parse` trait</span>

```console
$ aura run src/main.au
$12.00
Result.Ok($4.05)
Result.Err(`4.5` is not a price like 12.50)
Result.Err(`-3`: a price cannot be negative)
[$12.50, $3.00, $0.99]
average: 5.50
```

`Money.from(12)` reads as what it does: make a `Money` from the integer `12`,
meaning twelve dollars. Because `From` is a trait, the conversion has a
predictable name that every reader of the code will recognize, and it's the
same name `try` looks for when it has to convert an error.

`Money.parse` shows how conversions and errors fit together. Every way the text
can be wrong becomes a `Result.Err`, and the two `try parse_int64(...)` calls
pass the parser's own message up unchanged, because `parse` returns the same
`str` error type.

The most interesting line is in `parse_all`: `try T.parse(text)`. `T` is a type
parameter, and `T: Parse` promises it has an associated `parse` method, so the
generic function can call it through the type. `parse_all[Money]` uses
`Money.parse`; a `Port` or `Mode` type with its own `impl Parse` would get its
own. We have to specialize the call explicitly, as in `parse_all[Money](...)`,
because a list of strings says nothing about which type to parse into. As with
`Ord` in the previous section, builtin types don't come with our trait:
`parse_all[int64]` is rejected until someone writes `impl Parse for int64`.

Calling an associated method through a type parameter has a few restrictions in
Aura 0.11. The call has to be in a module-level function, not in a method, and
a function that makes it can't be started directly as a task. The
[Manual](/manual/generics-and-traits) lists them.

## Summary

Generics let you write a function, class, or enum once and use it with many
types. Type parameters are checked once, for every possible type, so the
ownership modes and operations in a generic body have to make sense for all of
them: `own T` for values the body keeps, and a trait bound for any method or
operator it calls. Traits name behavior, implementations promise it explicitly,
and bounds put the requirement in the signature where callers can read it. A
few traits connect to the language itself: `Display` controls printing, `Add`,
`Ord`, and friends give your types operators, and `From` lets `try` convert
errors.

Conversions follow the same philosophy of saying what you mean. `as` converts
numbers exactly or traps, `.to_float()` rounds on purpose, parsing text returns
a `Result`, and `str()` and f-strings render anything. Nothing converts behind
your back.

With error handling and generics in hand, you're ready to make sure your code
does what you think it does. [Chapter 11](/book/ch11-00-testing) introduces
`aura test`, and we'll use it to find a real bug in some code that looks
correct.
