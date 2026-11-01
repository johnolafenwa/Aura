# Functions

You have been writing functions since Chapter 1: every program so far has had
a `main`. Functions in Aura look almost exactly like Python functions. You
declare one with `def`, give it a name in `snake_case`, list its parameters in
parentheses, and write the body as an indented block. The difference is that
the types are part of the declaration.

Let's start the temperature program this chapter has been promising. Put this
in `src/main.au`:

*Filename: src/main.au*

```aura
def main():
    print(to_fahrenheit(100.0))
    print(to_fahrenheit(-40.0))

def to_fahrenheit(celsius: float64) -> float64:
    return celsius * 9.0 / 5.0 + 32.0
```

<span class="caption">Listing 3-3: A function that converts Celsius to Fahrenheit</span>

```console
$ aura run src/main.au
212.0
-40.0
```

`main` calls `to_fahrenheit` before the file defines it, and that's fine.
Aura reads the whole module before it checks any of it, so functions can
appear in any order and call each other freely. Unlike Python, there is no
need to put helpers above the code that uses them.

## Parameters

`to_fahrenheit` has one parameter, `celsius`, of type `float64`. Every
parameter must have a type. If you leave it out, the code doesn't even parse:

```aura check-fail:AU1101
def to_fahrenheit(celsius):
    return celsius * 9.0 / 5.0 + 32.0
```

```console
$ aura run src/main.au
error[AU1101]: expected Colon, found RParen
 --> src/main.au:1:26
  |
1 | def to_fahrenheit(celsius):
  |                          ^
```

The parser expected a `:` and a type after `celsius`, and found the closing
parenthesis instead. Inside a function, the compiler is happy to work out the
types of your bindings, but at the edges of a function it wants them written
down. This is a deliberate design decision. A function's signature is its
contract with every caller: if you can read the signature, you know what to
pass and what you'll get back, without reading the body. It also means the
compiler can check each function on its own. A mistake inside one function
produces an error in that function, rather than an error in some caller three
files away whose types were inferred from it.

The types are checked at every call:

```aura check-fail:AU2002
def to_fahrenheit(celsius: float64) -> float64:
    return celsius * 9.0 / 5.0 + 32.0

def main():
    print(to_fahrenheit(100))
    print(to_fahrenheit("hot"))
```

```console
$ aura run src/main.au
error[AU2002]: argument type mismatch for function `to_fahrenheit`: expected `float64`, found `str`
 --> src/main.au:6:25
  |
6 |     print(to_fahrenheit("hot"))
  |                         ^
```

The call on line 5 was accepted. `100` is an integer literal, but it is being
passed where a `float64` is expected, and `100` can be represented exactly as
a `float64`, so the literal takes that type. That's the same rule we saw with
`whole: float64 = 2` in the last section. It only works for literals; passing
a binding that holds an `int64` would be an error.

## Return values

The `-> float64` after the parameter list is the return type, and `return`
hands a value back to the caller. Aura has no implicit return. A function
that declares a return type must reach a `return` on every path through it.
If you come from Rust, where the last expression is the result, this is an
easy one to trip over:

```aura check-fail:AU2999
def to_fahrenheit(celsius: float64) -> float64:
    celsius * 9.0 / 5.0 + 32.0

def main():
    print(to_fahrenheit(100.0))
```

```console
$ aura run src/main.au
error[AU2999]: function `to_fahrenheit` is missing a return
 --> src/main.au:1:1
  |
1 | def to_fahrenheit(celsius: float64) -> float64:
  | ^
```

The same error catches a subtler mistake, a path through the function that
forgets to return:

```aura check-fail:AU2999
def describe(celsius: float64) -> str:
    if celsius < 0.0:
        return "freezing"
    elif celsius < 25.0:
        return "mild"

def main():
    print(describe(30.0))
```

```console
$ aura run src/main.au
error[AU2999]: function `describe` is missing a return
 --> src/main.au:1:1
  |
1 | def describe(celsius: float64) -> str:
  | ^
```

At `30.0` neither branch matches, and the function would fall off the end. In
Python, it would quietly return `None`, and the caller would find out later,
probably while trying to use that `None` as a string. Aura sees the missing
path at compile time. Adding an `else` branch, or a final `return` after the
`if`, fixes it.

A function with no `->` returns `None`. It can end by running off the end of
its body, or with a bare `return` to leave early:

```aura
def log_reading(celsius: float64):
    if celsius < -273.15:
        print("impossible reading")
        return
    print(f"reading: {celsius}")

def main():
    log_reading(20.0)
    log_reading(-300.0)
    result = log_reading(5.0)
    print(result)
```

```console
$ aura run src/main.au
reading: 20.0
impossible reading
reading: 5.0
None
```

Binding the result of `log_reading` is allowed, though not very useful: it is
the value `None`, which prints as `None`.

### Returning more than one value

To return several values, return a tuple. The return type lists the types of
its elements, and the caller unpacks it:

```aura
def min_max(readings: list[float64]) -> (float64, float64):
    mut low = readings[0]
    mut high = readings[0]
    for reading in readings:
        low = min(low, reading)
        high = max(high, reading)
    return (low, high)

def main():
    low, high = min_max([18.5, 22.0, 15.25, 19.0])
    print(f"low {low}, high {high}")
```

```console
$ aura run src/main.au
low 15.25, high 22.0
```

(`readings[0]` traps if the list is empty. In a real program you would decide
what an empty list should mean, which is what
[Chapter 9](/book/ch09-00-error-handling) is about.)

## Calling with names and defaults

Arguments can be passed by position, by name, or a mix of the two, and
parameters can have defaults, all as in Python. Listing 3-4 extends the
converter with a unit and a precision option.

*Filename: src/main.au*

```aura
def convert(value: float64, to_unit: str = "F", *, precise: bool = false) -> str:
    result = value * 9.0 / 5.0 + 32.0 if to_unit == "F" else value + 273.15
    if precise:
        return f"{result:.2f} {to_unit}"
    return f"{result:.0f} {to_unit}"

def main():
    print(convert(21.5))
    print(convert(21.5, "K"))
    print(convert(to_unit="K", value=21.5))
    print(convert(21.5, precise=true))
```

<span class="caption">Listing 3-4: Default values and keyword-only parameters</span>

```console
$ aura run src/main.au
71 F
295 K
295 K
70.70 F
```

`to_unit: str = "F"` gives `to_unit` a default, so callers can leave it out.
The `*` in the parameter list makes every parameter after it
*keyword-only*: `precise` can only be passed by name. That's a good habit for
`bool` options in particular, because `convert(21.5, "K", true)` doesn't tell
the reader what `true` means, while `precise=true` does. The compiler enforces
it:

```aura check-fail:AU2004
def convert(value: float64, to_unit: str = "F", *, precise: bool = false) -> str:
    return f"{value} {to_unit}"

def main():
    print(convert(21.5, "K", true))
```

```console
$ aura run src/main.au
error[AU2004]: parameter `precise` of function `convert` is keyword-only
 --> src/main.au:5:30
  |
5 |     print(convert(21.5, "K", true))
  |                              ^
```

The other rules are Python's too, and each mistake has its own message under
the same code, `AU2004`. Positional arguments must come before named ones
("positional arguments must come before named arguments in function
`convert`"). A name must match a parameter exactly, so `unit="K"` is rejected
with "function `convert` has no parameter named `unit`", which catches typos
that Python would only report when the call ran. And leaving out a parameter
that has no default gives "function `convert` is missing required argument
`value`".

Python developers know one trap with defaults: `def f(items=[])` creates the
list once, and every call shares it. Aura evaluates a default afresh on every
call, in parameter order, so that problem can't happen.

## Parameters borrow their arguments

There is one thing about parameters that has no equivalent in Python. Let's
try to write a function that hands back its unit unchanged:

```aura check-fail:AU3002
def unit_name(to_unit: str) -> str:
    return to_unit

def main():
    print(unit_name("F"))
```

```console
$ aura run src/main.au
error[AU3002]: parameter `to_unit` is borrowed; declare it as `own str` to take ownership, or clone the value before consuming it
 --> src/main.au:2:12
  |
2 |     return to_unit
  |            ^
  = related src/main.au:1:15: parameter `to_unit` is borrowed here
  = help: declare the parameter as `own str` when the function should consume it, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:2:19-2:19 with `.clone()`
```

This is the same kind of error as the one we met in the word counter. A plain
parameter, like `to_unit: str`, *borrows* its argument: the function may read
the string, but the caller still owns it. Returning it would hand the caller
ownership of a string that it already owns, which is the double-ownership
situation Aura exists to prevent. The `help` line gives the two ways out. The
function can return a copy, with `to_unit.clone()`, or it can declare that it
takes ownership of the argument with `to_unit: own str`. A third kind of
parameter, `mut`, lets the function change the caller's value, as
`count_words` did with its dictionary in Chapter 2.

Numbers and booleans don't have this problem, because they are copy values:
returning `celsius` from a function would return a copy of the number.
That's why everything else in this section worked without thinking about
ownership. Choosing between plain, `mut`, and `own` parameters is the heart of
[Chapter 4](/book/ch04-00-understanding-ownership), and it is how an Aura
signature tells you what a function takes and also what it does to it.

## Recursion

A function can call itself:

```aura
def factorial(n: int64) -> int64:
    if n <= 1:
        return 1
    return n * factorial(n - 1)

def main():
    print(factorial(20))
    print(factorial(21))
```

```console
$ aura run src/main.au
2432902008176640000
error[AU4002]: integer value `51090942171709440000` does not fit in `int64`
 --> src/main.au:4:12
  |
4 |     return n * factorial(n - 1)
  |            ^
  = note: Aura call chain (innermost first): factorial at 1:1 -> main at 6:1
```

`factorial(20)` fits in an `int64`. `factorial(21)` doesn't, and the overflow
check from the last section stops the program. The call chain in the note is
more interesting now: it lists the functions that were running, innermost
first, so you can see the multiplication failed inside `factorial`, which was
called from `main`.

## Lambdas

Sometimes a function is so small that giving it a name is more trouble than
it is worth, usually when you are passing it to something else. A *lambda* is
a function written inline as a single expression, with the same syntax as
Python's. You saw one in the word counter, as the sort key. Here are two
more:

```aura
def main():
    cities = ["Oslo", "Lagos", "Lima", "Reykjavik"]
    print(sorted(cities, key=lambda city: city.len()))
    offset = 273.15
    to_kelvin: def(float64) -> float64 = lambda celsius: celsius + offset
    print(to_kelvin(20.0))
```

```console
$ aura run src/main.au
[Oslo, Lima, Lagos, Reykjavik]
293.15
```

A lambda's parameters have no type annotations. Instead, the lambda takes its
types from where it is used. In the call to `sorted`, the compiler knows
`key` receives each element of a `list[str]`, so `city` is a `str`. For
`to_kelvin`, we wrote the type of the binding ourselves:
`def(float64) -> float64` is the type of a function that takes a `float64`
and returns a `float64`. Without that context, there is nothing to work from:

```aura check-fail:AU2002
def main():
    to_kelvin = lambda celsius: celsius + 273.15
```

```console
$ aura run src/main.au
error[AU2002]: lambda parameter types require an expected `def(...) -> ...` context
 --> src/main.au:2:17
  |
2 |     to_kelvin = lambda celsius: celsius + 273.15
  |                 ^
  = help: add a `def(...) -> ...` annotation to the immutable local, or pass the lambda directly to a callback parameter
```

The second lambda also uses `offset`, a binding from the surrounding
function. A lambda that does this is a *closure*: it captures the values it
uses when it is created. Closures, function types, and decorators are covered
in [Chapter 13](/book/ch13-00-closures-and-decorators). Until then, keep
lambdas short, and use a named `def` for anything that needs more than one
expression. The [Functions](/manual/functions) page of the Manual has the
complete rules for parameters, defaults, and calls.

Our converter is starting to grow, and a few of its choices deserve a note
for the next reader. That's what comments and docstrings are for.
