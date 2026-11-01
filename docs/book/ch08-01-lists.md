# Lists

A `list[T]` holds values of one type `T`, in order, and grows as you add to
it. It's the collection you'll reach for most, so it's where we'll start our
gradebook: one student's scores on a run of assignments.

*Filename: src/main.au*

```aura
def main():
    mut scores = [72, 95, 88]
    scores.append(64)
    print(scores)
    print(f"{scores.len()} scores, first {scores[0]}, last {scores[-1]}")
```

<span class="caption">Listing 8-1: Building a list, adding to it, and reading from both ends</span>

```console
$ aura run src/main.au
[72, 95, 88, 64]
4 scores, first 72, last 64
```

So far this is Python with a `mut` in front. The square-bracket literal makes
a list, `append` adds to the end, `len()` counts the elements (the builtin
`len(scores)` does the same thing), and indexing starts at zero. A negative
index counts back from the end, so `scores[-1]` is the last element.

The `mut` matters. `append` changes the list, and Aura only lets you change a
value through a mutable binding. Leave it off and the compiler points at the
call:

```aura check-fail:AU3003
def main():
    scores = [72, 95, 88]
    scores.append(64)
```

```console
$ aura check src/main.au
error[AU3003]: method `append` requires a mutable receiver
 --> src/main.au:3:12
  |
3 |     scores.append(64)
  |            ^
  = help: declare the receiver place with `mut` before calling a mutating method
```

## One element type

What type is `scores`? We didn't write one, so the compiler worked it out from
the literal: the elements are integer literals, so the list is a
`list[int64]`. Every element has to agree with that. A gradebook might be
tempted to record a missing assignment as a word:

```aura check-fail:AU2002
def main():
    scores = [72, "absent", 88]
```

```console
$ aura check src/main.au
error[AU2002]: list literal elements must all have type `int64`, found `str`
 --> src/main.au:2:19
  |
2 |     scores = [72, "absent", 88]
  |                   ^
```

Python would accept this list, and then some later line that adds up the
scores would fail when it reached the string. Because an Aura list has one
element type, any code that reads an element knows exactly what it will get,
and the compiler can check every use. When you really do have a value that may
be missing, say so in the type: a `list[int64 | None]` holds numbers or
`None`, and code reading it has to handle both, as you saw in
[Chapter 6](/book/ch06-00-enums-and-match).

The other side of inference is that an empty list gives the compiler nothing
to infer from:

```aura check-fail:AU2002
def main():
    scores = []
    print(scores)
```

```console
$ aura check src/main.au
error[AU2002]: empty list literals require an expected `list[T]` type annotation in the bootstrap compiler
 --> src/main.au:2:14
  |
2 |     scores = []
  |              ^
```

Give the binding a type, or call the list type as a constructor. Both of these
make an empty `list[int64]`:

```aura
def main():
    mut first: list[int64] = []
    mut second = list[int64]()
    first.append(1)
    second.append(2)
```

When you know roughly how many elements a list will end up with,
`list[int64].with_capacity(1_000)` makes an empty list with room for that
many, so it doesn't have to grow in steps as you append. The contents are the
same either way; only the amount of copying behind the scenes changes.

## Reading elements

Indexing is checked at run time. An index past the end doesn't wrap around or
return a placeholder; it stops the task:

```aura
def main():
    scores = [72, 95, 88]
    print(scores[3])
```

```console
$ aura run src/main.au
error[AU4003]: list index `3` is out of bounds for length `3`
 --> src/main.au:3:18
  |
3 |     print(scores[3])
  |                  ^
  = note: Aura call chain (innermost first): main at 1:1
```

This is a *trap*, a runtime failure that ends the task, and the program
exits with an error. Notice that `aura check` accepted the code: the compiler
doesn't try to prove indexes are in range, so the check happens when the line
runs. A trap is the right outcome when a bad index means the program has a
bug. [Chapter 9](/book/ch09-00-error-handling) talks about when to let a
program stop and when to recover.

When a missing element is an ordinary situation, not a bug, use `get`
instead. It returns a `Lookup[T]`, an enum with two variants, and `match`
makes you handle both:

*Filename: src/main.au*

```aura
def main():
    scores = [72, 95, 88]
    match scores.get(5):
        case Lookup.Found(score):
            print(f"sixth score: {score}")
        case Lookup.Missing:
            print("there is no sixth score")
```

<span class="caption">Listing 8-2: Reading an element that might not be there</span>

```console
$ aura run src/main.au
there is no sixth score
```

Aura uses a separate `Lookup` type here, not `int64 | None`, because a list
can hold `None` itself. In a `list[int64 | None]`, a `get` that returned
`None` couldn't tell you whether the element was missing or present and equal
to `None`. `Lookup.Missing` can only mean one thing.

Indexes are `int64`. A narrower integer, such as an `int32` position, widens
automatically when you use it as an index, because no `int32` value can be
out of the `int64` range. An unsigned `uint64` can, so it's refused with
`AU2002`, and you'd convert it with `as` first.

### Elements that aren't copy values

Our scores are copy values, so `scores[0]` hands you a copy of the number and
the list keeps its own. A list of strings behaves differently: `first =
names[0]` is an error, because it would create a second owner for a string
the list already owns. [Chapter 4](/book/ch04-00-understanding-ownership)
walked through that error and its three fixes: view the element in place with
`view first = names[0]`, make your own copy with `names[0].clone()`, or take
it out of the list with `pop`. Passing an element straight to a function, as
in `print(names[0])`, lends it for the call and needs none of them.

## Changing a list

Lists have the methods you'd expect, with names borrowed from Python. Let's
use most of them on our scores at once:

*Filename: src/main.au*

```aura
def main():
    mut scores = [72, 95, 88]
    scores.insert(0, 50)
    scores.extend([64, 91])
    print(scores)

    dropped = scores.pop(0)
    last = scores.pop()
    print(f"dropped {dropped} and {last}: {scores}")

    old = scores.set(1, 96)
    print(f"replaced {old}: {scores}")

    scores.remove(88)
    print(scores)
    print(f"64 is at {scores.index(64)}, and appears {scores.count(64)} time")
```

<span class="caption">Listing 8-3: Inserting, removing, and replacing elements</span>

```console
$ aura run src/main.au
[50, 72, 95, 88, 64, 91]
dropped 50 and 91: [72, 95, 88, 64]
replaced 95: [72, 96, 88, 64]
[72, 96, 64]
64 is at 2, and appears 1 time
```

`insert(position, value)` puts a value before `position`; like Python, it
clamps positions that are out of range, so `insert(100, x)` appends.
`extend` moves every element of another list onto the end. `pop()` removes
and returns the last element, and `pop(0)` the first. `set` is the one you may
not have met: it replaces an element and *returns the old one*. Assigning with
`scores[1] = 96` works too, but `set` hands the previous value back to you,
which matters when the elements are owned values you don't want to lose.
`remove(value)` deletes the first element equal to `value`, `index(value)`
finds its position, and `count(value)` counts how many there are.

`remove` and `index` have a sharp edge. If the value isn't there, they trap:

```aura
def main():
    mut scores = [72, 95, 88]
    scores.remove(50)
```

```console
$ aura run src/main.au
error[AU4008]: collection value was not found
 --> src/main.au
  = note: Aura call chain (innermost first): main at 1:1
  = help: check `value in values` before removing when absence is expected
```

The help line says what to do: when the value may legitimately be missing,
test first with `if 50 in scores:`. The `in` operator works on every
collection in this chapter. `pop` and `set` trap the same way when the
position is out of range, and `pop()` traps on an empty list.

Other changes you can make in place are `clear()`, which empties the list,
`reverse()`, and `swap(i, j)`. Sorting gets its own section below.

## Looping over a list

A `for` loop visits the elements in order. How it visits them depends on
what you ask for, and the three choices are the three access modes from
[Chapter 4](/book/ch04-00-understanding-ownership):

*Filename: src/main.au*

```aura
def main():
    mut scores = [72, 95, 64, 88]

    mut total: int64 = 0
    for score in scores:
        total += score
    print(f"total before curve: {total}")

    for score in mut scores:
        score += 3
    print(f"after curve: {scores}")

    for position, score in enumerate(scores):
        print(f"assignment {position + 1}: {score}")
```

<span class="caption">Listing 8-4: Reading, changing, and numbering elements in loops</span>

```console
$ aura run src/main.au
total before curve: 319
after curve: [75, 98, 67, 91]
assignment 1: 75
assignment 2: 98
assignment 3: 67
assignment 4: 91
```

A bare `for score in scores` gives the loop shared access: it reads each
element and leaves the list as it was. `for score in mut scores` gives it
mutable access, so `score += 3` changes the element inside the list, which is
how the curve takes effect. In Python, assigning to the loop variable would
change nothing but the variable. A third form, `for name in own names`, moves
each element out of the list and into the loop, consuming the list; after
that loop the list is gone, and using it is the familiar `AU3001`.

`enumerate(scores)` gives each element with its position, as a `(int64, T)`
pair, and `zip(first, second)` walks two lists side by side and stops at the
shorter one. Both are only valid as the thing a `for` loop iterates over.

While a bare loop is reading a list, the list is frozen. Here's an attempt to
add a make-up score for every failing grade while we're still looking at the
grades:

```aura check-fail:AU3002
def main():
    mut scores = [72, 95, 64, 88]
    for score in scores:
        if score < 70:
            scores.append(70)
```

```console
$ aura check src/main.au
error[AU3002]: cannot mutate `scores` while `scores` is borrowed for iteration
 --> src/main.au:5:20
  |
5 |             scores.append(70)
  |                    ^
  = related src/main.au:3:18: `scores` is borrowed for this loop here
  = help: perform owner mutation after the loop; use `for item in mut scores:` when mutating elements through the loop binding
```

In Python, appending to a list you're iterating over is a classic bug: the
loop may run forever or skip elements, depending on what you add and where.
Aura rules it out before the program runs. The two `related` and `help` lines
give the options: collect the changes and apply them after the loop, or use
`for score in mut scores` if what you want is to change each element in
place.

## Sorting

`sort()` puts a list in order, in place:

*Filename: src/main.au*

```aura
def last_name(name: str) -> str:
    parts = name.split(" ")
    return parts[-1].clone()

def main():
    mut names = ["Grace Hopper", "Ada Lovelace", "Alan Turing", "Katherine Johnson"]
    names.sort()
    print(names)
    names.sort(key=last_name)
    print(names)
    names.sort(key=lambda name: name.len(), reverse=true)
    print(names)
```

<span class="caption">Listing 8-5: Sorting naturally, by a key function, and in reverse</span>

```console
$ aura run src/main.au
[Ada Lovelace, Alan Turing, Grace Hopper, Katherine Johnson]
[Grace Hopper, Katherine Johnson, Ada Lovelace, Alan Turing]
[Katherine Johnson, Grace Hopper, Ada Lovelace, Alan Turing]
```

With no arguments, `sort` uses the natural order of the element type:
numbers by value, strings by their UTF-8 bytes. (That second rule means
uppercase letters sort before lowercase ones, so `"Zebra"` comes before
`"apple"`.) `key=` takes a function that computes a sort key for each
element, here a named function and then a lambda; the key is computed once
per element, before anything moves. `reverse=true` flips the order.

Look closely at the last line of output. Grace Hopper and Ada Lovelace have
names of the same length, and they stay in the order they had before the
sort, even with `reverse=true`. Aura's sort is *stable*: elements with equal
keys keep their relative order, in both directions. That lets you sort by
one thing and then another and keep the first order as a tiebreaker.

Natural sorting needs a type that has an order. A class of your own doesn't
have one until you give it one:

```aura check-fail:AU2002
class Student:
    name: str
    scores: list[int64]

def main():
    mut roster = [
        Student(name="Ada", scores=[72, 95]),
        Student(name="Grace", scores=[88]),
    ]
    roster.sort()
```

```console
$ aura check src/main.au
error[AU2002]: `list.sort` cannot order list element type `Student`
 --> src/main.au:10:12
   |
10 |     roster.sort()
   |            ^
  = help: use an existing naturally ordered type, or implement `Ord[T].lt` returning `bool`
```

Implementing the `Ord` trait is a topic for [Chapter
10](/book/ch10-00-generics-traits-conversions). The quick fix is a key:
`roster.sort(key=lambda student: student.name.clone())` sorts by name.

## Lists of richer values

Real gradebooks hold more than numbers. Let's give each student a name and a
list of scores, and see how a list of class values behaves:

*Filename: src/main.au*

```aura
class Student:
    name: str
    scores: list[int64]

def main():
    mut roster = [
        Student(name="Ada", scores=[72, 95]),
        Student(name="Grace", scores=[88]),
    ]
    roster[1].scores.append(91)
    match mut roster.lookup(0):
        case Lookup.Found(student):
            student.scores.append(64)
        case Lookup.Missing:
            print("no first student")
    for student in roster:
        print(f"{student.name}: {student.scores}")
    roster.sort(key=lambda student: student.scores.len())
    print(roster[0].name)
```

<span class="caption">Listing 8-6: Changing elements of a list of classes in place</span>

```console
$ aura run src/main.au
Ada: [72, 95, 64]
Grace: [88, 91]
Grace
```

`roster[1].scores.append(91)` reaches through the index to the student's
field and changes it where it lies, without copying the student out. That
works because `roster` is mutable and we're not trying to make a new owner,
only to change one that already exists.

The `match mut roster.lookup(0)` is the in-place cousin of `get`. `get` hands
you a *clone* of the element, which is fine for a number and wasteful for a
student with a long list of scores, and impossible for elements that can't be
cloned. `lookup` doesn't produce a value at all. It exists only as the subject
of a `match`: if the position is valid, the `Found` arm runs with `student`
naming the element inside the list, and with `match mut` the arm can change
it. While that arm runs, the list can't grow or shrink, so the view can't be
left pointing at nothing.

## Matching on a list's shape

`match` can look at a list's length and pull out its elements at the same
time. Here's a function that describes a student's progress in words:

*Filename: src/main.au*

```aura
def describe(scores: list[int64]) -> str:
    return match scores:
        case []: "no scores yet"
        case [only]: f"one score: {only}"
        case [first, second]: f"two scores: {first} and {second}"
        case [first, *rest]: f"{first}, then {len(rest)} more"

def main():
    print(describe([]))
    print(describe([88]))
    print(describe([88, 91]))
    print(describe([75, 98, 67, 91]))
```

<span class="caption">Listing 8-7: Sequence patterns</span>

```console
$ aura run src/main.au
no scores yet
one score: 88
two scores: 88 and 91
75, then 3 more
```

Each pattern matches lists of one exact length, except the last, where
`*rest` collects whatever the other positions leave into a new list. The arms
are tried in order, so the specific lengths go first. The
[Manual](/manual/enums-and-match) has the full rules for sequence patterns.

## Making new lists from old ones

Two list methods build a fresh list and leave the original alone. `filter`
keeps the elements a function accepts, and `map` transforms every element:

```aura
def passed(score: int64) -> bool:
    return score >= 70

def main():
    scores = [75, 98, 67, 91]
    print(scores.filter(passed))
    print(scores.map(lambda score: score * 10 // 9))
    print(scores)
```

```console
$ aura run src/main.au
[75, 98, 91]
[83, 108, 74, 101]
[75, 98, 67, 91]
```

Both run immediately and give you a complete list, not a lazy iterator. A
slice such as `scores[1:3]` also makes a new list, copying the elements it
selects; [Chapter 4](/book/ch04-00-understanding-ownership) covered how
slices work and what they cost. If you need to work on part of a list
*without* copying it, Aura has views of a range, called spans, which the
[Manual](/manual/collections) describes.

`map` and `filter` are fine, but for anything longer than one function call,
most Aura code uses comprehensions instead, along with builtins such as
`sorted` and `sum`. We'll get to those in [section
8.4](/book/ch08-04-comprehensions-and-builtins). First, though, our students
have names, and names are strings, which have more going on than they first
appear to.
