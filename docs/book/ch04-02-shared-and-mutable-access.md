# Shared and mutable access

At the end of the last section, our `total` function took ownership of the
list it added up. That was more than it needed. `total` only reads the
numbers, and once it has given us the sum it has no further use for the list.
Taking ownership forced the caller to give the list up or copy it first, and
neither makes sense for a function that only looks.

Most functions are like `total`. They want to read a value for the length of
one call, or change it in place, and then hand it straight back. Aura lets a
function say which of those it wants. There are three kinds of access, and
each one is spelled in the parameter's type:

- `values: list[int64]`, with no prefix, is *shared access*. The function can
  read the value. The caller keeps ownership.
- `values: mut list[int64]` is *mutable access*. The function can change the
  value in place. The caller keeps ownership and sees the changes.
- `values: own list[int64]` is *ownership transfer*. The value moves into the
  function, as we saw in the previous section.

Shared and mutable access are often called *borrowing*, because the function
has the value only for the duration of the call and then gives it back.

## Shared access

Let's take `own` off `total` and run the loop that failed before:

*Filename: src/main.au*

```aura
def total(values: list[int64]) -> int64:
    mut sum: int64 = 0
    for value in values:
        sum += value
    return sum

def main():
    scores = [10, 20, 30]
    for round in range(3):
        print(total(scores))
    print(scores)
```

<span class="caption">Listing 4-7: `total` borrows the list, so the caller can keep using it</span>

```console
$ aura run src/main.au
60
60
60
[10, 20, 30]
```

The call site did not change at all. It is still `total(scores)`, with no
marker to say "borrow this". In Aura the callee decides how it receives an
argument, and the caller reads that decision from the signature. A bare
parameter type means shared access for every type, whether it is an `int64`,
a `str`, a list, or one of your own classes. It is the default, and it is what
you should reach for first.

While `total` runs, it can read the list as much as it likes, and several
shared borrows of the same value can be live at once. What it cannot do is
change the list or take it. Let's try changing it.

## Mutating through shared access

Here is a function that adds a bonus score to the end of a list. We write the
parameter the same way as before:

*Filename: src/main.au*

```aura check-fail:AU3003
def add_bonus(values: list[int64]):
    values.append(5)

def main():
    mut scores = [10, 20, 30]
    add_bonus(scores)
    print(scores)
```

<span class="caption">Listing 4-8: Trying to append to a list we only have shared access to</span>

```console
$ aura check src/main.au
error[AU3003]: method `append` requires a mutable receiver
 --> src/main.au:2:12
  |
2 |     values.append(5)
  |            ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

The error is on line 2, inside `add_bonus`, at the call to `append`. A method
that changes a list needs a *mutable receiver*: the list it is called on has
to be something we are allowed to change. Inside `add_bonus`, `values` is a
shared borrow, so the answer is no. Notice that the binding in `main` was
declared `mut`. That doesn't matter here: the function's signature promised
read-only access, and the compiler holds the function to its signature.

This is the rule that most surprises Python programmers, because Python has
no such promise. Any function you pass a list to can append to it, sort it,
or clear it, and the only way to know is to read the function's body (and the
bodies of everything it calls). A lot of Python code defends itself by copying
lists before passing them anywhere, to be safe. In Aura a bare parameter is
a guarantee. If `total` takes `values: list[int64]`, then after
`total(scores)` returns, `scores` holds exactly what it held before. You can
know that from one line of the signature.

## Mutable access

To let `add_bonus` change the caller's list, we say so in the parameter type
with `mut`:

*Filename: src/main.au*

```aura
def add_bonus(values: mut list[int64]):
    values.append(5)

def main():
    mut scores = [10, 20, 30]
    add_bonus(scores)
    print(scores)
```

<span class="caption">Listing 4-9: Taking mutable access lets the function change the caller's list</span>

```console
$ aura run src/main.au
[10, 20, 30, 5]
```

The append happened to the list that `main` owns. Nothing was copied in or
out: `values` inside `add_bonus` is another way to reach `scores` for the
length of the call.

This time the binding's `mut` does matter. Let's remove it:

```aura check-fail:AU2004
def add_bonus(values: mut list[int64]):
    values.append(5)

def main():
    scores = [10, 20, 30]
    add_bonus(scores)
    print(scores)
```

```console
$ aura check src/main.au
error[AU2004]: argument for parameter `values` in function `add_bonus` must be a mutable place
 --> src/main.au:6:15
  |
6 |     add_bonus(scores)
  |               ^
```

A *place* is somewhere a value lives: a binding, a field, a list slot. A
mutable place is one declared with `mut`, or a field of one. Since `scores`
was declared without `mut`, `main` has said that this list never changes, and
the compiler won't let a call quietly break that promise. The two halves fit
together. The function declares that it will change its argument, and the
caller declares that its binding may change. When you read `mut scores =` at
the top of a function, you know to look for something that modifies it.

## Mutable access is exclusive

Shared borrows can overlap freely, since nobody is changing anything. Mutable
access has one more rule: while a value is borrowed mutably, nothing else may
reach it at the same time, not even for reading. Here is a function that
appends every element of one list onto another, called with the same list for
both:

*Filename: src/main.au*

```aura check-fail:AU3002
def merge(into: mut list[int64], source: list[int64]):
    for value in source:
        into.append(value)

def main():
    mut scores = [10, 20, 30]
    merge(scores, scores)
    print(scores)
```

<span class="caption">Listing 4-10: Passing one list as both a mutable and a shared argument</span>

```console
$ aura check src/main.au
error[AU3002]: argument for parameter `source` in function `merge` overlaps mutable borrow for parameter `into`; mutable borrows must be exclusive
 --> src/main.au:7:19
  |
7 |     merge(scores, scores)
  |                   ^
  = related src/main.au:7:11: mutable borrow for parameter `into` begins here
  = help: pass non-overlapping places; shared accesses may overlap, but mutable access must remain exclusive
```

Read the two locations together. The `related` line says that a mutable
borrow for `into` begins at column 11, the first `scores`. The arrow points
at column 19, the second `scores`, which tries to share the same list as
`source` while that mutable borrow is still live.

Think about what `merge` would do with one list behind both names. It loops
over `source` and appends each element to `into`. Every append makes `source`
longer, so the loop never reaches its end. Python shows exactly this
behavior. If you write

```python
scores = [10, 20, 30]
for value in scores:
    scores.append(value)
```

the loop runs until the program runs out of memory. Aura refuses the same
loop written directly, for the same reason:

```aura check-fail:AU3002
def main():
    mut scores = [10, 20, 30]
    for value in scores:
        scores.append(value)
    print(scores)
```

```console
$ aura check src/main.au
error[AU3002]: cannot mutate `scores` while `scores` is borrowed for iteration
 --> src/main.au:4:16
  |
4 |         scores.append(value)
  |                ^
  = related src/main.au:3:18: `scores` is borrowed for this loop here
  = help: perform owner mutation after the loop; use `for item in mut scores:` when mutating elements through the loop binding
```

A `for` loop borrows the list it walks over for the whole loop, and the
append in the body tries to change it while that borrow is live. The help
line gives the two usual fixes: collect the new values somewhere else and add
them after the loop, or, if what you wanted was to change each element, use
`for ... in mut`, which we'll see shortly.

Infinite loops are the friendly outcome. In a language without a garbage
collector, growing a list can move its storage to a bigger block of memory,
and anything still pointing into the old block is now pointing at freed
memory. C++ programmers know this as *iterator invalidation*. The exclusivity
rule rules out the whole family of bugs at once: if a value can change, only
one piece of code can be looking at it. The same rule is what later lets Aura
run tasks in parallel without data races, which we'll come back to in
[Chapter 15](/book/ch15-00-concurrency).

## Borrowed fields stay where they are

Shared access lets a function read a value's fields, but not take them away.
Here is a function that tries to return a user's name from a borrowed
`User`:

*Filename: src/main.au*

```aura check-fail:AU3002
class User:
    name: str
    age: int64

def name_of(user: User) -> str:
    return user.name

def main():
    user = User(name="Ada", age=36)
    print(name_of(user))
```

```console
$ aura check src/main.au
error[AU3002]: cannot move non-copy field `name` out of borrowed value `user`
 --> src/main.au:6:17
  |
6 |     return user.name
  |                 ^
  = related src/main.au:5:13: `user` is borrowed here
  = help: take `user` as `own User` when the field should be moved, or call `.clone()` on the field to return an independent value
  = fix: replace src/main.au:6:21-6:21 with `.clone()`
```

Returning a value from a function moves it to the caller, as we saw with
`make_roster`. But `name_of` doesn't own `user.name`. The caller does, and it
expects to get its `User` back intact when the call ends. Returning the field
would move the string out of someone else's value.

The help line lists the two honest options, and they mean different things.
Cloning the field returns a new string and leaves the caller's `User`
untouched. Taking `own User` consumes the whole user and moves the name out of
it, which is the right shape for a function that converts one value into
another. Here are both in one program:

*Filename: src/main.au*

```aura
class Player:
    name: str
    score: int64

def name_of(user: Player) -> str:
    return user.name.clone()

def into_name(user: own Player) -> str:
    return user.name

def main():
    player = Player(name="Ada", score=90)
    print(name_of(player))
    print(player.score)
    print(into_name(player))
```

```console
$ aura run src/main.au
Ada
90
Ada
```

After `into_name(player)`, `player` is gone. Fields that are copy values,
like `score`, are never a problem: returning `user.score` from a shared borrow
returns a copy.

## Receivers follow the same rules

Methods take their instance through a *receiver*, the `self` at the front of
the parameter list, and it comes in the same three forms. Bare `self` is
shared access, `mut self` is mutable access, and `own self` takes ownership.
Here is a small scoreboard class that uses all three:

*Filename: src/main.au*

```aura
class Scoreboard:
    scores: list[int64]

    def best(self) -> int64:
        mut top: int64 = 0
        for value in self.scores:
            if value > top:
                top = value
        return top

    def record(mut self, value: int64):
        self.scores.append(value)

    def curve(mut self, points: int64):
        for value in mut self.scores:
            value += points

    def into_scores(own self) -> list[int64]:
        return self.scores

def main():
    mut board = Scoreboard(scores=[70, 85])
    board.record(92)
    print(board.best())
    board.curve(5)
    scores = board.into_scores()
    print(scores)
```

<span class="caption">Listing 4-11: A class whose methods read, change, and consume the instance</span>

```console
$ aura run src/main.au
92
[75, 90, 97]
```

`best` only reads, so it takes `self`. `record` and `curve` change the list,
so they take `mut self`, and the instance has to live in a mutable place:
drop the `mut` from `mut board` and the call to `record` fails the same way
`add_bonus` did, with an error that points at `.record`:

```console
$ aura check src/main.au
error[AU3003]: method `record` requires a mutable receiver
 --> src/main.au:23:11
   |
23 |     board.record(92)
   |           ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

`into_scores` takes `own self`, so calling it consumes `board`. If we add a
`print(board.best())` at the end of `main`, the compiler points at both the
use and the call that moved it:

```console
$ aura check src/main.au
error[AU3001]: use of moved value `board`
 --> src/main.au:28:11
   |
28 |     print(board.best())
   |           ^
  = related src/main.au:26:14: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

The help mentions that `Scoreboard` is non-cloneable. Classes don't get a
copy method automatically, so the only way forward is to call `into_scores`
last. We'll spend more time on receivers in
[Chapter 5](/book/ch05-00-classes).

## Changing elements in a loop

`curve` uses a loop form we haven't seen yet. A plain `for value in scores`
borrows the list and gives you each element with shared access, so you can't
assign to `value`:

```aura check-fail:AU3003
def main():
    scores = [10, 20, 30]
    for value in scores:
        value += 1
```

```console
$ aura check src/main.au
error[AU3003]: cannot assign to immutable binding `value`; declare it with `mut` to rebind it
 --> src/main.au:4:9
  |
4 |         value += 1
  |         ^
```

In Python this loop would run and do nothing visible, since `value += 1`
rebinds the loop variable and leaves the list alone. Aura offers the version
you probably meant. Writing `mut` before the list makes each `value` a
mutable view of the element itself, so assignments land in the list. And
writing `own` before the list consumes it, handing you each element as an
owned value:

*Filename: src/main.au*

```aura
def main():
    mut scores = [10, 20, 30]
    for value in mut scores:
        value *= 2
    print(scores)
    for value in own scores:
        print(value)
```

<span class="caption">Listing 4-12: `for ... in mut` changes elements in place; `for ... in own` consumes the list</span>

```console
$ aura run src/main.au
[20, 40, 60]
20
40
60
```

The `mut` loop needs a mutable list, for the same reason a `mut` parameter
does. After the `own` loop, `scores` has moved into the loop and can't be
used again. Notice where the keyword goes. On a parameter it sits in the type,
as in `values: mut list[int64]`, because a parameter has a type position. A
loop has no type to annotate, so the keyword goes in front of the thing being
iterated.

## Choosing an access mode

When you write a function, ask what it does with each argument, and pick the
weakest access that does the job:

1. If it only reads, use a bare type. This is the common case, and it leaves
   the caller free to keep using the value.
2. If it changes the caller's value in place, use `mut`. The caller will need
   a `mut` binding, which makes the change visible at both ends.
3. If it keeps the value (storing it in a list or a field, returning it, or
   taking it apart), use `own`. The caller gives it up, or passes a copy.

Python lets every function do all three to everything it's given. Aura asks
you to write down which one you mean, and in return every signature tells the
reader, and the compiler, what happens to the arguments. The
[Manual](/manual/ownership-and-borrowing) has the complete rules, including
views, returned views, and borrowed pattern matching.

There is one situation we keep running into, where we really do want a
second, independent value: two owners, each free to change its own. In the
next section we'll look at copies and slices, and at what they cost.
