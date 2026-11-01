# Appendix C: Aura for Python developers

Aura was designed so that Python programmers would feel at home in it, and on
the surface they do. Blocks are indented, functions start with `def`, classes
with `class`, strings with f-strings work the way you expect, and so do
comprehensions, keyword arguments, `for x in items`, `with`, and chained
comparisons. That familiarity is useful, but it also means the differences
arrive as surprises: code that looks right and does not compile. This
appendix collects those differences in one place, with the reason for each,
so that the first week with Aura is less of a negotiation with the compiler.

If you have read the book from the start, you have already met all of these.
Treat this as the cheat sheet you hand a Python colleague.

## Scripts and `main`

A file of top-level statements is a script, and runs from top to bottom with
no ceremony:

```aura
langs = ["python", "aura"]

for lang in langs:
    print(f"hello, {lang}")
```

There is no `if __name__ == "__main__":`. When you want an entry point,
usually because you want an exit status or a compiled binary, you write a
`main` function. Its `int32` result becomes the process exit status:

```aura
def main() -> int32:
    print("hello")
    return 0
```

A file chooses one style or the other. Mixing top-level statements with a
`main` is an error (`AU2999`: "files cannot mix top-level statements ... with
an explicit `main` function"). Declarations such as `class` and `def` are
fine in both.

One small thing trips people up on the first day: `print` takes exactly one
argument. `print(a, b)` is `AU2004`. Use an f-string, `print(f"{a} {b}")`.

## Bindings are immutable by default

In Python every name can be rebound. In Aura a binding is fixed unless you
say otherwise, and this is usually the first error a Python programmer sees:

```aura check-fail:AU3003
def main():
    total = 0
    total = total + 1
```

```console
$ aura check immut.au
error[AU3003]: cannot assign to immutable binding `total`; declare it with `mut` to rebind it
 --> immut.au:3:5
  |
3 |     total = total + 1
  |     ^
```

The fix is one word:

```aura
def main():
    mut total = 0
    total = total + 1
    print(total)
```

The same rule covers mutating a collection. `names.append("b")` needs
`mut names = [...]`, because appending changes the list. Aura asks you to
mark mutation because then you can read a function and see at a glance which
values it changes. It also lets the ownership checker reason about who can
see a change, which is the next topic.

## Values have one owner

This is the biggest difference, and the one worth the most patience.
Python passes references to objects and lets a garbage collector decide when
nobody needs them. Aura has no garbage collector. Every value has exactly one
owner, and a function signature says what the function does with what you
pass it:

| Parameter | The function may | Afterwards, the caller |
| --- | --- | --- |
| `name: str` | read it | still owns it |
| `name: mut str` | change it | still owns it, changed |
| `name: own str` | keep it, change it, drop it | no longer has it |

Call sites look like Python: no `&`, no sigils. The compiler reads the
signature and does the right thing. When you give a value away with `own`, it
holds you to that:

```aura check-fail:AU3001
def consume(name: own str) -> int64:
    return len(name)


def main():
    label = "aura"
    n = consume(label)
    print(label)
```

```console
$ aura check owner.au
error[AU3001]: use of moved value `label`
 --> owner.au:7:11
  |
7 |     print(label)
  |           ^
  = related owner.au:6:17: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()` at the move site when an independent value is required
  = fix: replace owner.au:6:22-6:22 with `.clone()`
```

Read an error like this as a question: did you mean to hand `label` over, or
only to let `consume` look at it? If the function only reads, drop the `own`.
If you really need two independent values, make a copy at the call. Strings
copy with `.clone()`, and lists, dicts, and sets copy with `.copy()`.
Classes have neither method, deliberately: if you want a second instance,
build one.

Numbers, `bool`, `Duration`, and copy classes (declared `copy class`) copy
automatically, so you can pass them around freely. Strings, collections,
ordinary class instances, and resources such as files move. Chapter 4 covers
all of this in depth.

A related surprise: indexing a list of non-copy values does not give you an
independent value, because that would quietly copy or move it out of a list
you still own. `first = names[0]` on a `list[str]` is `AU3005`, and the
message names the alternatives: `view first = names[0]` for an alias,
`names[0].clone()` for a copy, or `names.pop(0)` to take it out.

## Types are static, locals are inferred

Parameters and return types must be annotated. Everything else is inferred:

```aura
def total(prices: list[float64]) -> float64:
    mut sum = 0.0
    for price in prices:
        sum += price
    return sum
```

Three consequences catch people. First, `def f(x):` does not parse, because
an untyped parameter is not a thing Aura has. Second, a function without
`->` returns `None`, and Aura does not infer a return type from the body:

```aura check-fail:AU2002
def double(n: int64):
    return n * 2
```

The compiler reports `return type mismatch: expected None, found int64`; add
`-> int64`. Third, numbers never convert implicitly. An unsuffixed integer
literal is `int64` and a float literal is `float64`. Passing an `int32` where
an `int64` is expected, or adding the two, is an error. Convert with
`value as int64`, or `.to_float()` for a float.

## No truthiness, no integer `/`

`if values:` is an error, because conditions must be `bool`. Write what you
mean: `if len(values) > 0:` or `if value is not None:`. Python's rule that
empty things are false is convenient until the day `0` or `""` turns out to be
a valid value, and Aura would rather you spell out which case you are
testing.

Integer `/` is rejected with `AU2003`. Python 3 made `7 / 2` produce `3.5`,
while most other languages produce `3`. Rather than pick one and surprise half
its users, Aura makes you choose: `//` for floor division, or `.to_float()`
on both sides for true division. `//` and `%` round the same way as in
Python.

## Failure is a return value

Aura has no exceptions, so there is no `raise`, `try`/`except`, or
`finally`. A function that can fail says so in its return type with
`Result[T, E]`, and the caller has to deal with both outcomes:

```aura
def parse_port(text: str) -> Result[int64, str]:
    port = try parse_int64(text)
    if port < 1 or port > 65535:
        return Result.Err(f"port out of range: {port}")
    return Result.Ok(port)


def main():
    match parse_port("8080"):
        case Result.Ok(port):
            print(f"listening on {port}")
        case Result.Err(message):
            print(f"bad config: {message}")
```

`try` in Aura is an expression, not a block: `try parse_int64(text)` returns
the `Ok` value, or returns the `Err` from the enclosing function at once. It
does the job that letting an exception propagate does in Python, but you can
see every place it happens.

"A value or nothing" is `T | None`, such as `str | None`, tested with
`is None` and `is not None`. The compiler narrows the type after the test, so
there is no `Optional` unwrapping to do by hand.

Some failures are not recoverable values: an index out of range, a failed
`assert`, integer overflow, division by zero. Those *trap*, stopping the task
with an `AU40xx` diagnostic and an Aura call chain, much like an uncaught
exception would. The difference is that you cannot catch a trap, so it is
reserved for bugs, not for conditions a program should handle.

## Classes, without inheritance

A class is a set of fields and methods. Without an `__init__`, you construct
it by naming its fields, and fields may have defaults:

```aura
class Account:
    owner: str
    balance: float64
    currency: str = "USD"

    def new(owner: own str) -> Account:
        return Account(owner=owner, balance=0.0)

    def deposit(mut self, amount: float64):
        self.balance += amount

    @property
    def summary(self) -> str:
        return f"{self.owner}: {self.balance} {self.currency}"


impl Display for Account:
    def display(self) -> str:
        return f"<account {self.owner}>"


def main():
    mut account = Account.new("ada")
    account.deposit(25.0)
    print(account.summary)
    print(account)
```

A few things in that listing replace Python habits:

- `new` has no `self`, which makes it an associated function, called as
  `Account.new(...)`. It is the Aura answer to `@classmethod` constructors,
  and a class can have as many as it likes.
- The receiver says what a method does to its object, with the same words as
  parameters: `self` reads, `mut self` changes, `own self` consumes. Writing
  `self.balance += amount` in a method declared with plain `self` is
  `AU3003`, and the help line tells you to write `mut self`.
- `@property` works much as in Python, for read-only computed values. It is
  one of the few decorators allowed on a method.
- `__str__` becomes an implementation of the `Display` trait. After that,
  `print(account)`, `str(account)`, and `f"{account}"` all use it.

`__init__` exists too, when construction needs logic, and `with` works with
classes that define `__exit__(mut self)` (Chapter 14).

`class Dog(Animal):` does not parse. Aura has no inheritance. Shared behavior
is a trait that several types implement (Chapter 10), and shared data is a
field holding another class. If you have spent time untangling deep class
hierarchies in Python, you may not miss it.

## Strings

`str` is Unicode, and `len(text)` counts Unicode scalar values. Strings are
not indexable: `text[0]` is an error, but slices such as `text[1:4]` work and
return a new string. Most methods have the names you expect, and a few have
Aura names:

| Python | Aura |
| --- | --- |
| `s.strip()`, `s.lstrip()`, `s.rstrip()` | `s.trim()`, `s.trim_start()`, `s.trim_end()` |
| `s.upper()`, `s.lower()` | `s.to_upper()`, `s.to_lower()` |
| `s.split()` | `s.split_whitespace()` |
| `s.splitlines()` | `s.lines()` |
| `s.partition(sep)` | `s.split_once(sep)`, which returns `(str, str) \| None` |
| `s.find(sub)` | `s.find(sub)`, which returns `int64 \| None` instead of `-1` |
| `s * 3` | `s.repeat(3)` |
| `int(s)` | `parse_int64(s)`, which returns a `Result` |

The pattern in the last three rows is the theme of the whole language: where
Python returns a sentinel or raises, Aura returns a type that makes the
missing case impossible to ignore.

## `is` means `is None`

`value is None` and `value is not None` are the only uses of `is`. There is
no identity comparison between arbitrary objects, and `a is b` is a syntax
error. Use `==` for equality.

## Concurrency and processes

Python offers threads, `asyncio`, and `multiprocessing`, each with its own
rules. Aura has one model: tasks started inside a `with group = TaskGroup():`
block, values passed between them through `Queue[T]`, and cancellation that
tasks observe at well-defined waiting points. There is no `async` or `await`
to color your functions, and no global lock: tasks really do run on several
cores. The price is that values crossing between tasks must move, never be
shared, which the compiler checks (Chapter 15).

`process.run` takes a list of arguments and never runs a shell, unlike
`subprocess.run(..., shell=True)`. If you want shell syntax, say so with
`["/bin/sh", "-c", command]`, as the job runner in Chapter 19 does.

## Module state

Top-level bindings in a file with `main` are immutable module storage. A
`mut` binding at the top level is rejected with `AU3003`, and moving a value
out of one is `AU3001`. Mutable state belongs to an owner, which is usually `main`
or an object it creates. This rules out the module-level caches and counters
that Python code tends to accumulate, and with them a whole class of
"who changed this?" bugs.

## Where to go next

Chapter 4 is the one to read slowly, because ownership is the idea that does
not map onto anything in Python. After that, the
[Manual](/manual/) is the reference for exact rules, and the diagnostics in
[Appendix E](/book/appendix-05-common-diagnostics) are the ones you will meet most as you
make the switch.
