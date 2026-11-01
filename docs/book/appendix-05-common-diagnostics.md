# Appendix E: Diagnostics you will meet often

Every message `aura` prints starts with a stable code such as `AU3001`. The
first digit after `AU` tells you which part of the compiler is talking: `1`
for reading the text of your program, `2` for names and types, `3` for
ownership and borrowing, and `4` for traps at run time. The code stays the
same from release to release even when the wording improves, so it is the
thing to search for.

A diagnostic has the same parts every time. The first line is the code and a
one-line message. The `-->` line gives the file, line, and column, and the
excerpt underneath marks the exact spot with `^`. Then come optional records:
`related` points at a second place that explains the first (where a value
moved, where a borrow started), `note` adds context, `help` suggests what to
do, and `fix` is an exact replacement that an editor can apply for you. When
there is a `related` line, read it; it is usually the half of the story you
were missing.

This appendix collects the diagnostics that people learning Aura run into
most, each with a minimal program that triggers it, the real output, and the
usual fix. The [Diagnostics](/manual/diagnostics) page of the Manual has the
complete table of codes.

## Reading your source

### `AU1001`: tabs and unclosed delimiters

`AU1001` means the compiler could not even split your file into tokens. The
two common causes are tab characters used for indentation and a bracket that
is never closed. Aura requires spaces for indentation, because a file that
mixes tabs and spaces can look correctly indented in one editor and wrongly
indented in another:

```console
$ aura check tabs.au
error[AU1001]: tabs are not supported for indentation; use spaces
 --> tabs.au:2:1
  |
2 | 	print(1)
  | ^
```

Configure your editor to insert spaces for `.au` files, or run `aura fmt`
once the file parses. An unclosed bracket is reported at the end of the file,
with a `related` line pointing at the bracket that was opened:

```aura check-fail:AU1001
def main():
    print("a"
```

```console
$ aura check unclosed.au
error[AU1001]: unclosed delimiter `(`; expected `)` before end of file
 --> unclosed.au:3:1
  = related unclosed.au:2:10: opening delimiter `(` is here
```

### `AU1101`: a missing parameter type

`AU1101` is the general syntax error. The one Python programmers hit first is
a parameter without a type:

```aura check-fail:AU1101
def double(x):
    return x * 2
```

```console
$ aura check untyped.au
error[AU1101]: expected Colon, found RParen
 --> untyped.au:1:13
  |
1 | def double(x):
  |             ^
```

The parser expected the `:` that starts a type annotation and found `)`
instead. Every parameter needs a type: `def double(x: int64) -> int64:`.
The same code appears when you use a keyword such as `class` or `match` as a
name; Appendix A lists them.

## Names and types

### `AU2001`: unknown name

```aura check-fail:AU2001
def main():
    count = 3
    print(cuont)
```

```console
$ aura check unknown.au
error[AU2001]: unknown name `cuont`
 --> unknown.au:3:11
  |
3 |     print(cuont)
  |           ^
```

Usually a typo, or a name from a module you have not imported. Remember that
module members are qualified: after `import fs`, the function is
`fs.read_to_string`, not `read_to_string`, unless you wrote
`from fs import read_to_string`.

### `AU2002`: type mismatch

`AU2002` covers any value of the wrong type in a position that requires a
specific one. Two versions come up constantly. The first is mixing integer
widths, because Aura never converts numbers implicitly:

```aura check-fail:AU2002
def wait_for(seconds: int64):
    print(seconds)


def main():
    n: int32 = 5
    wait_for(n)
```

```console
$ aura check argtype.au
error[AU2002]: argument type mismatch for function `wait_for`: expected `int64`, found `int32`
 --> argtype.au:6:14
  |
6 |     wait_for(n)
  |              ^
```

Convert at the call with `wait_for(n as int64)`, or change one of the types
so they agree. The second is a function that returns a value but forgot to
say so. A missing `->` means the function returns `None`:

```aura check-fail:AU2002
def double(n: int64):
    return n * 2
```

```console
$ aura check retmismatch.au
error[AU2002]: return type mismatch: expected `None`, found `int64`
 --> retmismatch.au:2:5
  |
2 |     return n * 2
  |     ^
```

Add the return type: `def double(n: int64) -> int64:`. You will also see
`AU2002` when you use a `T | None` value as if it were a `T`. The fix there is
to test it with `is None` first, which narrows the type.

### `AU2003`: integer division with `/`

```aura check-fail:AU2003
def average(total: int64, count: int64) -> int64:
    return total / count
```

```console
$ aura check intdiv.au
error[AU2003]: integer `/` is not supported; use `//` for floor division, or call `.to_float()` on both operands for true division
 --> intdiv.au:2:12
  |
2 |     return total / count
  |            ^
```

The message names both fixes. Use `total // count` when you want an integer
result rounded down, and `total.to_float() / count.to_float()` with a
`float64` return type when you want the exact quotient. Aura makes you pick
because the two meanings differ between languages, and guessing wrong
produces numbers that are subtly off.

### `AU2004`: arguments that do not fit the parameters

```aura check-fail:AU2004
def main():
    name = "ada"
    print("hello", name)
```

```console
$ aura check args.au
error[AU2004]: `print` expects 1 argument, found 2
 --> args.au:3:5
  |
3 |     print("hello", name)
  |     ^
```

`AU2004` means the arguments at a call do not bind to the callee's parameter
list: too many, too few, or a named argument that does not exist. With
`print`, the fix is an f-string: `print(f"hello {name}")`.

### `AU2005`: Python spellings

When you write something that is valid Python but not Aura, the compiler
often recognizes it and says what to write instead:

```aura check-fail:AU2005
def main():
    done = True
```

```console
$ aura check pyshape.au
error[AU2005]: unknown name `True`; did you mean `true`?
 --> pyshape.au:2:12
  |
2 |     done = True
  |            ^
```

The same code gives targeted advice for `try:`/`except:` (use `Result` and
`match`), for `str(...)`-style constructors, and for other habits that carry
over from Python.

### `AU2999`: a match that misses a case

Every `match` must handle every possible value. Leave out an enum variant and
the compiler names it:

```aura check-fail:AU2999
enum Light:
    Red
    Amber
    Green


def action(light: Light) -> str:
    match light:
        case Light.Red:
            return "stop"
        case Light.Green:
            return "go"
```

```console
$ aura check missingarm.au
error[AU2999]: non-exhaustive match over `Light`: missing `Amber`
 --> missingarm.au:7:5
  |
7 |     match light:
  |     ^
```

Add the missing arm, or a `case _:` when the remaining variants really do
share one answer. Prefer naming them: when someone adds a fourth variant
later, a match without a wildcard sends them to every place that needs a
decision. Matching on a union type such as `int64 | str` has its own code,
`AU2013`, with a message like `match does not cover str`; the fix is the
same.

### `AU2999`: a function that is missing a return

```aura check-fail:AU2999
def sign(n: int64) -> str:
    if n < 0:
        return "negative"
    elif n > 0:
        return "positive"
```

```console
$ aura check noreturn.au
error[AU2999]: function `sign` is missing a return
 --> noreturn.au:1:1
  |
1 | def sign(n: int64) -> str:
  | ^
```

Some path through the body reaches the end without returning, here when `n`
is zero. The compiler does not reason about which conditions are possible,
and it does not treat `while true:` as a loop that never ends, so a loop that
only exits through `return` gets this error too. Return on the last path, or
restructure so the loop exits with `break` and the function returns after it,
as Chapter 19 does in `run_job`.

## Ownership and borrowing

### `AU3003`: changing something that is not mutable

```aura check-fail:AU3003
def main():
    count = 0
    count += 1
```

```console
$ aura check immutable.au
error[AU3003]: cannot assign to immutable binding `count`; declare it with `mut` to rebind it
 --> immutable.au:3:5
  |
3 |     count += 1
  |     ^
```

Declare the binding with `mut count = 0`. The same code appears when you call
a mutating method such as `append` on a binding or parameter that is not
`mut`, and when a method changes `self` without declaring `mut self`:

```aura check-fail:AU3003
def add(items: list[str]):
    items.append("x")
```

```console
$ aura check mutparam.au
error[AU3003]: method `append` requires a mutable receiver
 --> mutparam.au:2:11
  |
2 |     items.append("x")
  |           ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

Here the fix is `items: mut list[str]`, which also tells every caller that
`add` changes the list they pass in.

### `AU3001`: use of a moved value

```aura check-fail:AU3001
def consume(name: own str):
    print(name)


def main():
    name = "ada"
    consume(name)
    print(name)
```

```console
$ aura check moved.au
error[AU3001]: use of moved value `name`
 --> moved.au:7:11
  |
7 |     print(name)
  |           ^
  = related moved.au:6:13: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()` at the move site when an independent value is required
  = fix: replace moved.au:6:17-6:17 with `.clone()`
```

`consume` takes `own str`, so the call gave the string away, and the last
line tries to use it. Decide which side is wrong. If `consume` only needs to
read, change its parameter to `name: str`. If it really needs to keep the
value, and you need one too, pass a copy: `.clone()` for a `str`, `.copy()`
for a list, dict, or set. Starting a task with `group.start_soon(f, value)`
also moves `value`, which surprises people the first time; read anything you
need from it before the call, as the job runner in Chapter 19 does with
`total = len(jobs)`.

### `AU2999`: a move inside a loop

The same mistake inside a loop gets a different code, because the problem is
not a later use but the next iteration:

```aura check-fail:AU2999
def consume(name: own str):
    print(name)


def main():
    name = "ada"
    for _ in range(3):
        consume(name)
```

```console
$ aura check loopmove.au
error[AU2999]: `for` loop body moves `name` and may execute more than once
 --> loopmove.au:6:5
  |
6 |     for _ in range(3):
  |     ^
```

The first iteration moves `name` into `consume`, and the second would have
nothing to pass. Clone inside the loop with `consume(name.clone())`, or
change `consume` to borrow.

### `AU3002`: moving out of something you only borrowed

`AU3002` is the borrow checker's general complaint: you tried to take
ownership of something that is only lent to you. Two forms come up again and
again. The first is moving a field out of a borrowed object:

```aura check-fail:AU3002
class Job:
    name: str


def keep(name: own str):
    print(name)


def report(job: Job):
    keep(job.name)
```

```console
$ aura check fieldmove.au
error[AU3002]: cannot move non-copy field `name` out of borrowed value `job`
 --> fieldmove.au:8:14
  |
8 |     keep(job.name)
  |              ^
  = related fieldmove.au:7:12: `job` is borrowed here
  = help: take `job` as `own Job` when the field should be moved, or call `.clone()` on the field to return an independent value
  = fix: replace fieldmove.au:8:18-8:18 with `.clone()`
```

`report` has shared access to `job`, so it cannot hand a piece of it to
`keep`. Clone the field, or take `job: own Job` if `report` is meant to use
the job up.

The second is moving a value out of a plain `match`, which only borrows the
value it inspects:

```aura check-fail:AU3002
def first_word(text: str) -> Result[str, str]:
    words = text.split_whitespace()
    if len(words) == 0:
        return Result.Err("empty")
    return Result.Ok(words[0].clone())


def keep(word: own str):
    print(word)


def main():
    match first_word("hello world"):
        case Result.Ok(word):
            keep(word)
        case Result.Err(message):
            print(message)
```

```console
$ aura check sharedmatch.au
error[AU3002]: cannot move `word` out of a shared match on `first_word(...)`
 --> sharedmatch.au:13:18
   |
13 |             keep(word)
   |                  ^
  = related sharedmatch.au:12:24: value is borrowed here
  = help: write `match own first_word(...)` to consume the scrutinee, or call `.clone()` to consume an independent copy
  = fix: replace sharedmatch.au:13:22-13:22 with `.clone()`
```

When the matched value is a temporary that nothing else needs, as here, write
`match own first_word("hello world"):` and the arm owns `word`. A `for` loop
over a list has the same rule: `for job in jobs:` lends each element, and
`for job in own jobs:` hands them over.

### `AU3005`: taking an element out of a list by index

```aura check-fail:AU3005
def main():
    names = ["ada", "grace"]
    first = names[0]
    print(first)
```

```console
$ aura check index.au
error[AU3005]: cannot implicitly copy `str` out of a list index; use `view value = items[index]` for shared access, `items[index].clone()` for an explicit copy, or `pop(index)` or `set(index, value)` to transfer ownership
 --> index.au:3:13
  |
3 |     first = names[0]
  |             ^
```

`first = names[0]` would either copy the string or move it out of a list that
still owns it, and Aura does neither silently. The message lists your
choices. For reading, `view first = names[0]` gives a name to the element in
place. For an independent value, `names[0].clone()`. To take it out of the
list, `pop(0)` on a `mut` list. Indexing a list of copy values, such as
`list[int64]`, works as you would expect.

### `AU3008`: a value that cannot cross into a task

```aura check-fail:AU3008
import random


def roll(rng: own random.Rng) -> int64:
    return 1


def main():
    rng = random.Rng(seed=1)
    with group = TaskGroup():
        task = group.start(roll, rng)
```

```console
$ aura check transfer.au
error[AU3008]: task argument `rng` cannot cross a task boundary because `random.Rng` is a stateful generator and is not Transfer
 --> transfer.au:9:34
  |
9 |         task = group.start(roll, rng)
  |                                  ^
  = related transfer.au:3:10: task parameter `rng` is declared here
  = help: send owned data made only from Transfer components; keep capabilities and host resources on their owning worker
```

Values passed to a task, returned from one, or sent through a `Queue` must be
plain owned data: numbers, strings, collections, and classes built from
those, plus queue and task handles. Live resources (open files, child
processes, sockets), random generators, and task groups stay on the task that
created them. The fix is to send the data instead of the resource: a seed
instead of a generator, a path instead of an open file.

## Traps at run time

Traps are failures the compiler cannot rule out in advance, so they appear
when you `aura run` a program that `aura check` accepted. They stop the task,
print an `AU40xx` diagnostic with an `Aura call chain` note showing how the
program got there, and make `aura run` exit with status 1. Output the program
printed before the trap is kept.

### `AU4003`: index out of bounds

```aura
def main():
    names = ["ada", "grace"]
    view third = names[2]
    print(third)
```

```console
$ aura run bounds.au
error[AU4003]: list index `2` is out of bounds for length `2`
 --> bounds.au:3:24
  |
3 |     view third = names[2]
  |                        ^
  = note: Aura call chain (innermost first): main at 1:1
```

Check the length first, or use `names.get(2)`, which returns
`Lookup.Found(value)` or `Lookup.Missing` instead of trapping. `AU4003` also
covers slice endpoints outside the string or list, and an empty separator
passed to `split_once`.

### `AU4002`: integer overflow

```aura
def main():
    mut small: int8 = 120
    small += 10
    print(small)
```

```console
$ aura run overflow.au
error[AU4002]: integer value `130` does not fit in `int8`
 --> overflow.au:3:5
  |
3 |     small += 10
  |     ^
  = note: Aura call chain (innermost first): main at 1:1
```

Integer arithmetic is checked in Aura, so a result outside the type's range
traps rather than wrapping around to a wrong number. Use a wider type, or the
explicit `wrapping_add` and `saturating_add` family when wrapping is what you
want.

### `AU4001`: failed assertions and other invalid states

`AU4001` is the general runtime trap. You will see it most often from a
failed `assert`, where the message is the text you supplied and notes show
the values on each side of a comparison (Appendix D has an example from
`aura test`). It also reports invalid values passed to the runtime, such as a
negative timeout: Chapter 19 runs into exactly that with
`get(timeout=...) must be non-negative`, and fixes it by clamping the value
to zero.
