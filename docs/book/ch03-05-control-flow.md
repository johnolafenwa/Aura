# Control flow

Programs make decisions and repeat work. Aura's tools for both are the ones
you know from Python: `if`, `elif`, and `else` for decisions, and `while` and
`for` for loops, with `break` and `continue` to leave a loop early or skip
ahead. What needs your attention is the handful of places where Aura is
stricter than Python. Once we've seen those, we'll finish the temperature
program with a function that prints a conversion table.

## `if`, `elif`, and `else`

An `if` runs its block when its condition is true. `elif` adds another
condition to try when the earlier ones were false, and `else` catches
everything left:

```aura
def describe(celsius: float64) -> str:
    if celsius < 0.0:
        return "freezing"
    elif celsius < 15.0:
        return "cold"
    elif celsius < 25.0:
        return "mild"
    else:
        return "hot"

def main():
    print(describe(-5.0))
    print(describe(18.0))
    print(describe(31.0))
    temperature = 18.0
    print(10.0 <= temperature < 20.0)
```

```console
$ aura run src/main.au
freezing
mild
hot
true
```

The conditions are tried in order, and only the first true one runs. Because
`describe` has an `else`, every path returns a string, which the compiler
checks, as we saw in [Functions](/book/ch03-03-functions).

The last line shows a *chained comparison*: `10.0 <= temperature < 20.0`
means `10.0 <= temperature and temperature < 20.0`, with `temperature`
evaluated only once. It works the way it does in Python and reads the way
you'd write it in math.

There are two rules to remember. The first is the one from
[Data types](/book/ch03-02-data-types): a condition must be a `bool`. Aura
has no truthiness, so `if count:` is an error and you write `if count > 0:`.
The second is that `=` is never a comparison. Writing `if x = 3:` is a syntax
error ("expected Colon, found Equal"), so the classic bug of assigning when
you meant to compare can't get past the parser. Use `==`.

When an `if` only chooses between two values, the conditional expression
`value if condition else other` is usually clearer, and as we saw in
[Bindings and mutability](/book/ch03-01-bindings-and-mutability), it also
avoids the problem of a binding that only exists inside one branch. Both
sides must have the same type.

When a block has to exist but there's nothing to do in it, write `pass`, as in
Python.

## Repeating with `while`

A `while` loop runs its block for as long as its condition stays true:

```aura
def main():
    mut celsius = -10
    while celsius <= 30:
        print(celsius)
        celsius += 20
```

```console
$ aura run src/main.au
-10
10
30
```

The binding the loop changes has to be `mut`, of course. Forgetting that gives
the `AU3003` error from earlier in the chapter, and it's the most common
mistake with `while` loops.

When the reason to stop is easier to express in the middle of the loop than
at the top, write `while true:` and leave with `break`. That's the shape the
word counter used to read lines until the end of the input:

```aura
def main():
    mut tries = 0
    while true:
        tries += 1
        if tries == 3:
            break
    print(tries)
```

## Looping over a collection with `for`

`for` visits each element of a collection in turn. It's the loop you'll use
most often, because it can't go out of bounds and can't loop forever.
Listing 3-6 averages a list of sensor readings, and uses `continue` and
`break` to deal with bad data.

*Filename: src/main.au*

```aura
def main():
    readings = [21.5, -999.0, 19.0, 22.5, 120.0, 18.0]
    mut total = 0.0
    mut count = 0
    for reading in readings:
        if reading == -999.0:
            continue
        if reading > 100.0:
            print("sensor fault, stopping")
            break
        total += reading
        count += 1
    print(f"{count} readings, average {total / count.to_float():.2f}")
```

<span class="caption">Listing 3-6: Skipping and stopping inside a `for` loop</span>

```console
$ aura run src/main.au
sensor fault, stopping
3 readings, average 21.00
```

`-999.0` is a "no reading" marker, so `continue` skips straight to the next
element. `120.0` is impossible for this sensor, so `break` leaves the loop
altogether, and the last reading, `18.0`, is never seen. Three readings were
counted: `21.5`, `19.0`, and `22.5`. Notice the `count.to_float()` in the
last line. `count` is an integer and `total` is a float, so they can't be
divided directly.

The loop binding, `reading` here, is new on each pass and belongs to the
loop's block, so it doesn't exist after the loop ends. The loop borrows each
element from the list rather than taking it out. For numbers that makes no
difference, because they are copy values. For strings and other owned values
it matters, which is why, in the word counter, we
could read `word` but not store it in a dictionary. The ownership chapter
shows the other two forms, `for x in mut items` to change elements in place
and `for x in own items` to take them out of the list.

When you need the position as well as the element, use `enumerate`, as in
Python:

```aura
def main():
    cities = ["Oslo", "Lagos", "Lima"]
    for index, city in enumerate(cities):
        print(f"{index + 1}. {city}")
```

```console
$ aura run src/main.au
1. Oslo
2. Lagos
3. Lima
```

`zip(first, second)` walks two lists side by side in the same way.

### Counting with `range`

To loop a fixed number of times, loop over a `range`. `range(stop)` counts
from `0` up to, but not including, `stop`, and `range(start, stop)` starts
somewhere else:

```aura
def main():
    for i in range(3):
        print(i)
    for i in range(5, 8):
        print(i)
    for i in range(3, 0):
        print(i)
    for i in reversed(range(1, 4)):
        print(i)
```

```console
$ aura run src/main.au
0
1
2
5
6
7
3
2
1
```

A range whose start is not below its stop is empty, so `range(3, 0)` runs
zero times and prints nothing. To count down, wrap the range in `reversed`.
What Aura's `range` doesn't have yet is Python's third argument, the step:

```aura check-fail:AU2004
def main():
    for n in range(0, 10, 2):
        print(n)
```

```console
$ aura run src/main.au
error[AU2004]: `range` expects 1 or 2 arguments, found 3
 --> src/main.au:2:14
  |
2 |     for n in range(0, 10, 2):
  |              ^
```

This is a gap in the current preview rather than a design decision. Until it
is filled, use a `while` loop with the step written out, as the `celsius`
loop above did, or multiply: `for i in range(5):` with `n = i * 2` inside.

Python's `for ... else` and `while ... else` clauses don't exist in Aura
either. An `else:` after a loop is a syntax error. If you need to know
whether a loop finished without a `break`, keep a `mut found = false` binding
and set it before you break.

## Expressions over several lines

A long expression can continue onto the next line as long as a parenthesis,
square bracket, or brace is still open. That covers long function calls,
long signatures, and list or dictionary literals, which can also end with a
trailing comma:

```aura
def main():
    total = (
        1.5
        + 2.5
    )
    print(total)
    message = describe_range(
        -10.0,
        40.0,
    )
    print(message)

def describe_range(low: float64, high: float64) -> str:
    return f"{low} to {high}"
```

```console
$ aura run src/main.au
4.0
-10.0 to 40.0
```

Without an open bracket, the end of the line ends the statement. A trailing
operator doesn't continue the line, and there's no backslash continuation:

```aura check-fail:AU1101
def main():
    total = 1.5 +
        2.5
    print(total)
```

```console
$ aura run src/main.au
error[AU1101]: unexpected token in expression: Newline
 --> src/main.au:2:18
  |
2 |     total = 1.5 +
  |                  ^
```

The fix is to put the expression in parentheses, as in the first example. The
indentation of the continued lines is up to you, because it doesn't open a
new block; one extra level of four spaces is the usual choice.

## Putting it together: a conversion table

Let's finish the temperature program. Listing 3-7 prints a table of Celsius
temperatures, their Fahrenheit equivalents, and a description, using most of
what this chapter has covered.

*Filename: src/main.au*

```aura
def to_fahrenheit(celsius: float64) -> float64:
    """Convert a Celsius reading to Fahrenheit."""
    return celsius * 9.0 / 5.0 + 32.0

def describe(celsius: float64) -> str:
    """Return a one-word description of a Celsius temperature."""
    if celsius < 0.0:
        return "freezing"
    elif celsius < 15.0:
        return "cold"
    elif celsius < 25.0:
        return "mild"
    else:
        return "hot"

def print_table(start: int64, stop: int64, *, step: int64 = 10):
    """Print Celsius, Fahrenheit, and a description from `start` up to `stop`.

    Args:
        start: The first Celsius value in the table.
        stop: The last Celsius value; included if the steps land on it.
        step: The distance between rows.
    """
    print(f"{'C':>6} {'F':>7}  description")
    mut celsius = start
    while celsius <= stop:
        value = celsius.to_float()
        print(f"{celsius:>6} {to_fahrenheit(value):>7.1f}  {describe(value)}")
        celsius += step

def main():
    print_table(-20, 40)
    print("")
    print_table(0, 30, step=15)
```

<span class="caption">Listing 3-7: A temperature conversion table</span>

```console
$ aura run src/main.au
     C       F  description
   -20    -4.0  freezing
   -10    14.0  freezing
     0    32.0  cold
    10    50.0  cold
    20    68.0  mild
    30    86.0  hot
    40   104.0  hot

     C       F  description
     0    32.0  cold
    15    59.0  mild
    30    86.0  hot
```

`print_table` takes whole-number bounds and a keyword-only `step` with a
default, documented with a docstring. Its loop is a `while` because it needs
a step other than one. Each row converts the integer `celsius` to a float once
with `.to_float()`, and passes that value to both helpers. The f-string
formats every column to a fixed width, and even calls functions inside the
braces. One small difference from Python shows up in `main`: `print` always
takes exactly one argument, so a blank line is `print("")` rather than
`print()`.

A few things to try if you want to keep going: add a Kelvin column, make
`describe` take its thresholds as parameters with defaults, or print the
table in reverse by counting down from `stop`.

## Summary

You now know the everyday core of Aura. Bindings are immutable unless you
write `mut`, they live until the end of their block, and they keep the type
they started with. Integers, floats, booleans, strings, `None`, and tuples
cover most values, nothing converts between types unless you ask, and
overflow and division by zero stop the program instead of producing a wrong
answer. Functions spell out their parameter and return types, can take
arguments by name and have defaults, and must return on every path. Comments
and docstrings document your code for readers and for your editor. `if`,
`while`, `for`, and `range` give you decisions and loops, with a `bool`
required for every condition.

Several times in this chapter we ran into ownership: a string that couldn't be
stored because it was borrowed, a parameter that couldn't be returned, a list
that couldn't change because its binding wasn't `mut`. Those all come from
the one idea that makes Aura different from Python, and it's the subject of
the next chapter,
[Understanding ownership](/book/ch04-00-understanding-ownership).
