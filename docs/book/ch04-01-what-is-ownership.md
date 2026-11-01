# What is ownership

Let's start with a Python program that surprises almost everyone the first
time they meet it:

```python
scores = [10, 20, 30]
backup = scores
backup.append(40)
print(scores)   # [10, 20, 30, 40]
```

The line `backup = scores` does not copy anything. It makes a second name for
the same list, so appending through `backup` changes what `scores` sees. Python
is happy with two names for one object because it never has to decide which
name is responsible for the list. The runtime keeps the list alive as long as
any name can still reach it and frees it some time after the last one goes
away.

Aura has no runtime doing that bookkeeping, so it needs a different rule. The
rule is that every value has exactly one *owner*: one binding, one field, one
list slot, or one function parameter that is responsible for it. When the
owner goes away, so does the value. Most of this chapter is about what
happens when you assign, pass, or return a value under that rule.

## Copy values

Some values are so small and so simple that "who owns this?" has an obvious
answer: everyone gets their own. Integers are the clearest case. Here is a
program that assigns one integer binding to another and then changes the
second:

*Filename: src/main.au*

```aura
def main():
    count = 3
    mut other = count
    other += 1
    print(count)
    print(other)
```

<span class="caption">Listing 4-1: Assigning a copy value gives the new binding its own copy</span>

```console
$ aura run src/main.au
3
4
```

`other = count` copied the number 3 into `other`. The two bindings have
nothing to do with each other after that, so adding one to `other` leaves
`count` alone. This is what you would expect from Python too, for a different
reason: Python integers are immutable, so `other += 1` makes `other` point at
a new integer object. In Aura the integer itself was duplicated.

We call types that behave this way *copy values*. The built-in ones are the
integer types (`int64`, `int32`, `uint8`, and the rest), `float32` and
`float64`, `bool`, and `Duration`. A tuple is a copy value when every element
is, so `(3, 4)` copies but `("Ada", 4)` does not. Later chapters add a few
more: you can declare your own copy classes in
[Chapter 5](/book/ch05-00-classes), and enums whose payloads are all copy
values copy too. What these types have in common is a small, fixed size with
nothing behind them in memory, which makes a copy as cheap as reading the
value in the first place.

## Owned values and moves

Everything else is an *owned value*. A `str` holds its characters in memory
that has to be released when the string is done. A `list[int64]` owns a block
of memory that grows as you append. Instances of your own classes, files,
network connections, and task groups all hold something that needs cleaning
up. Copying those on every assignment would be expensive and, for a file or a
socket, would not even make sense.

So assigning an owned value does not copy it. It *moves* it: ownership passes
from the old binding to the new one, and the old binding can no longer be
used. Let's see what that looks like when we forget:

*Filename: src/main.au*

```aura check-fail:AU3001
def main():
    name = "Ada"
    other = name
    print(other)
    print(name)
```

<span class="caption">Listing 4-2: Using a string after it has moved</span>

```console
$ aura check src/main.au
error[AU3001]: use of moved value `name`
 --> src/main.au:5:11
  |
5 |     print(name)
  |           ^
  = related src/main.au:3:13: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()` at the move site when an independent value is required
  = fix: replace src/main.au:3:17-3:17 with `.clone()`
```

The first line tells us what went wrong: we used `name` after it moved. The
arrow points at line 5, the `print(name)` that the compiler refused. The
`related` line points back at line 3, column 13, which is the `name` in
`other = name`. That is where ownership left. Between them, the two locations
tell the whole story: the string moved into `other` on line 3, and on line 5
we tried to read it through a binding that no longer owns anything.

The `help` line offers two ways out. The first, "pass shared access", is
about functions and is the subject of the
[next section](/book/ch04-02-shared-and-mutable-access). The second is to make
an independent copy at the place where the move happened. The `fix` line even
says exactly where to type it: right after `name` on line 3.

*Filename: src/main.au*

```aura
def main():
    name = "Ada"
    other = name.clone()
    print(other)
    print(name)
```

<span class="caption">Listing 4-3: Cloning the string so that both bindings own one</span>

```console
$ aura run src/main.au
Ada
Ada
```

Now there are two strings, each with its own owner. We'll look at copying in
detail, including what it costs, in
[Copies and slices](/book/ch04-03-copies-and-slices).

### Why a move invalidates the old binding

It is fair to ask why Aura doesn't do what Python does and let both names
point at the same string. The answer comes from what an owner is for.

When a binding goes out of scope, Aura releases what it owns. If `name` and
`other` both owned the same string, both would try to release it at the end
of `main`. The second release would hand back memory that was already given
back and possibly already reused for something else. That bug has a name in C
and C++, a *double free*, and it is one of the classic ways to corrupt a
program's memory. A milder version of the same problem is a *use after free*:
one owner releases the string, and the other keeps reading memory that now
belongs to someone else.

Python sidesteps both by never letting your code release anything. The
reference count on the string tracks how many names point at it, and the
interpreter frees the string only when that count reaches zero. That works,
but it has costs: every assignment updates a counter, objects that refer to
each other in a cycle wait for a separate collector pass, and you cannot say
from the source code exactly when an object is freed.

Aura takes the opposite route. Because exactly one owner exists at any
moment, the compiler always knows who will release a value and where. A move
updates that knowledge at compile time, so a moved-from binding is not "a
second pointer the runtime has to track". It is a name that the compiler
knows is empty, and using it is a mistake it can report before you run
anything. No counter is updated at run time, and no collector ever runs.

## Moving into a function

Assignment is not the only way to move a value. Passing it to a function can
move it too, when the function says so. Here is a function that adds up a
list of scores and takes ownership of the list to do it:

*Filename: src/main.au*

```aura check-fail:AU3001
def total(values: own list[int64]) -> int64:
    mut sum: int64 = 0
    for value in values:
        sum += value
    return sum

def main():
    scores = [10, 20, 30]
    print(total(scores))
    print(scores)
```

<span class="caption">Listing 4-4: Using a list after handing it to a function that takes ownership</span>

Most languages would let this through. Aura won't:

```console
$ aura check src/main.au
error[AU3001]: use of moved value `scores`
 --> src/main.au:10:11
   |
10 |     print(scores)
   |           ^
  = related src/main.au:9:17: value moved here
  = help: pass shared access when ownership is not needed, or call `.copy()` at the move site when an independent value is required
  = fix: replace src/main.au:9:23-9:23 with `.copy()`
```

The message names both places that matter: the `print` on line 10 that tried
to use `scores`, and the call on line 9 where it moved into `total`. The word
`own` in front of the parameter type is what made the call a move. Because
`total` asked for `own`, it now owns the list and is free to drop it when it
returns. If Aura let us print `scores` after that, we would be reading memory
that no longer belongs to us. Python avoids the problem with a garbage
collector that keeps the list alive as long as anyone can reach it. Aura has
no garbage collector, so it makes the hand-off visible in the signature
instead, and checks it before the program runs.

Notice that the call site looks exactly like any other call: `total(scores)`.
You don't mark a move where it happens. The signature of `total` carries that
information, which is one reason Aura signatures are worth reading carefully.

This time the help line suggests `.copy()` instead of `.clone()`. Lists copy
with `.copy()`, and strings with `.clone()`, and the compiler picks the right
spelling for the type it found. Passing `scores.copy()` would hand `total` a
fresh list and keep the original. The better fix here is not to make `total`
take ownership at all, since it only reads the list. That is where the next
section starts.

### Moves inside loops

The compiler also thinks about code that runs more than once. If a loop body
moves a value that was created outside the loop, the second time around there
is nothing left to move:

*Filename: src/main.au*

```aura check-fail:AU2999
def total(values: own list[int64]) -> int64:
    mut sum: int64 = 0
    for value in values:
        sum += value
    return sum

def main():
    scores = [10, 20, 30]
    for round in range(3):
        print(total(scores))
```

```console
$ aura check src/main.au
error[AU2999]: `for` loop body moves `scores` and may execute more than once
 --> src/main.au:9:5
  |
9 |     for round in range(3):
  |     ^
```

Python would run this three times and print `60` three times. In Aura, the
first iteration would give the list to `total`, which drops it, and the
second would pass a list that no longer exists. The compiler doesn't wait to
see how many times the loop really runs: it rejects any move in a loop body
that could happen twice.

## Giving ownership back

Moves go in the other direction too. When a function returns an owned value,
ownership passes to whoever called it:

*Filename: src/main.au*

```aura
def make_roster() -> list[str]:
    mut roster = list[str]()
    roster.append("Ada")
    roster.append("Grace")
    return roster

def main():
    roster = make_roster()
    print(roster)
```

<span class="caption">Listing 4-5: Returning a list moves it to the caller</span>

```console
$ aura run src/main.au
[Ada, Grace]
```

`make_roster` builds a list, owns it while it fills it, and then moves it out
through `return`. The binding in `main` becomes the new owner. Nothing is
copied on the way, which is why returning a large list from a function is
cheap. Every function that returns `-> T` hands its caller an owned value in
this way.

The same rule applies when you put an owned value into a collection.
`roster.append(name)` moves `name` into the list, and the list is its owner
from then on. Printing `name` after that line gives the same "use of moved
value" error we saw in Listing 4-2, with a `related` line pointing at the
`append`.

## Moving fields out of a class

A class instance owns its fields, and each field is tracked on its own. You
can move one owned field out of an instance and keep using the others. Here
`Player` has a `str` field and an `int64` field. (We define classes properly
in [Chapter 5](/book/ch05-00-classes); for now, read `class Player:` as "a
record with these two fields".)

*Filename: src/main.au*

```aura check-fail:AU3001
class Player:
    name: str
    score: int64

def main():
    player = Player(name="Ada", score=90)
    name = player.name
    print(name)
    print(player.score)
    print(player.name)
```

```console
$ aura check src/main.au
error[AU3001]: use of moved field `name` from `player`
 --> src/main.au:10:18
   |
10 |     print(player.name)
   |                  ^
  = related src/main.au:7:19: field moved here
  = help: use shared access to the field when ownership is not needed, or call `.clone()` before moving it when an independent value is required
```

Line 7 moved the string out of `player.name` into the new binding `name`.
Reading `player.score` on line 9 is fine, because `score` is an `int64` and
reading it copies. Line 10 is the problem: the `name` field is empty now. If
we try to print the whole `player` instead, the compiler calls it a
*partially moved* value, since one of its fields is gone:

```aura check-fail:AU3001
class Player:
    name: str
    score: int64

def main():
    player = Player(name="Ada", score=90)
    name = player.name
    print(player)
```

```console
$ aura check src/main.au
error[AU3001]: use of partially moved value `player`
 --> src/main.au:8:11
  |
8 |     print(player)
  |           ^
  = related src/main.au:7:19: field moved here
  = help: use shared access to the field when ownership is not needed, or call `.clone()` before moving it when an independent value is required
```

A partially moved instance becomes whole again if you assign a new value to
the moved field, which needs a `mut` binding. More often you will want to read
the field without taking it, and that is what shared access is for.

## Scope and cleanup

An owner's life is bounded by its scope. A binding created inside a block
belongs to that block, and it is gone when the block ends:

*Filename: src/main.au*

```aura check-fail:AU2001
def main():
    scores = [10, 20, 30]
    if scores.len() > 2:
        best = 30
        print(best)
    print(best)
```

```console
$ aura check src/main.au
error[AU2001]: unknown name `best`
 --> src/main.au:6:11
  |
6 |     print(best)
  |           ^
```

Python would accept this, because a Python name assigned inside an `if` lives
on in the whole function. Aura binds `best` to the `if` block. That is not
only a tidiness rule: the end of a scope is where owned values are released.
When `main` returns, `scores` goes out of scope and its memory is handed back
right there, not at some later point chosen by a collector.

For memory, that difference is mostly about predictability. For resources it
matters a great deal. A file that closes when its owner leaves scope closes at
a line you can point to. In Python you get the same guarantee only if you
remember to write `with`. Aura uses `with` for resources too, and the block
ends at the same point every time. Here is a small class that prints when it
is opened and closed, so we can watch the order:

*Filename: src/main.au*

```aura
class Logbook:
    name: str

    def __enter__(mut self) -> None:
        print(f"open {self.name}")

    def __exit__(mut self) -> None:
        print(f"close {self.name}")

def main():
    print("start")
    with book = Logbook(name="scores"):
        print("writing")
    print("after the block")
```

<span class="caption">Listing 4-6: Cleanup runs exactly when the `with` block ends</span>

```console
$ aura run src/main.au
start
open scores
writing
close scores
after the block
```

`close scores` appears before `after the block`, every time, whatever the
block does. If the body traps partway through, the cleanup still runs on the
way out. We will build on this in
[Chapter 14](/book/ch14-00-resources-and-cleanup), where files, processes, and
sockets all follow the same pattern.

## A note about top-level bindings

There is one place where the rules above look different, and it catches
people writing short scripts. A binding at the top level of a file, outside
any function, is *module storage* (the module constants of
[Chapter 3](/book/ch03-01-bindings-and-mutability)). It belongs to the module for the whole run
of the program, and you cannot move out of it:

*Filename: src/main.au*

```aura check-fail:AU3001
def total(values: own list[int64]) -> int64:
    return values.len()

scores = [10, 20, 30]

def main():
    print(total(scores))
```

```console
$ aura check src/main.au
error[AU3001]: cannot move `scores` out of immutable module storage
 --> src/main.au:7:17
  |
7 |     print(total(scores))
  |                 ^
  = help: call `.copy()` when an independent owned value is required
```

Module storage has to stay valid for every function that might read it, so
letting one call take it away would leave the others reading nothing. The fix
is the one the compiler suggests (pass `scores.copy()`) or, more often, to
create the value inside `main`. That is why the examples in this chapter keep
their values in `main`.

So far, every time a function wanted to look at a value we had to choose
between giving the value away and making a copy. Neither is what we want most
of the time. In the next section we'll let functions borrow values instead.
