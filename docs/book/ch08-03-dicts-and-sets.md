# Dictionaries and sets

A list answers "what's at position 3?". Most questions a gradebook asks are
different: "what are Grace's scores?", "has Alan handed anything in?". For
questions by name, you want a `dict[K, V]`, which maps keys of type `K` to
values of type `V`. For questions of membership alone, you want a `set[T]`.
Both work much as they do in Python, and both follow the ownership rules of
the last section, with one twist for keys that's worth slowing down for.

## Creating and reading a dictionary

*Filename: src/main.au*

```aura
def main():
    mut grades = {"ada": 72, "grace": 95}
    grades["alan"] = 64
    grades["ada"] = 78
    print(grades)
    print(grades["grace"])
    print(f"{grades.len()} students")
    print("kathy" in grades)
    match grades.get("kathy"):
        case Lookup.Found(grade):
            print(f"kathy: {grade}")
        case Lookup.Missing:
            print("no grade for kathy")
    match grades.remove("alan"):
        case Lookup.Found(grade):
            print(f"removed alan, who had {grade}")
        case Lookup.Missing:
            print("alan was not there")
    print(grades)
```

<span class="caption">Listing 8-12: Adding, updating, reading, and removing entries</span>

```console
$ aura run src/main.au
{ada: 78, grace: 95, alan: 64}
95
3 students
false
no grade for kathy
removed alan, who had 64
{ada: 78, grace: 95}
```

The literal `{"ada": 72, "grace": 95}` makes a `dict[str, int64]`, inferred
from the keys and values the same way a list's element type is. Every key has
to have the key type and every value the value type; mixing them is an
`AU2002`, as it is for lists. An empty dictionary needs its type written
down, either as `grades: dict[str, int64] = {}` or as `dict[str, int64]()`.

Assigning to `grades["alan"]` adds a new entry, and assigning to an existing
key, as with `"ada"`, replaces its value. A dictionary keeps its keys in the
order they were first inserted, and updating a value doesn't move its key,
which is why `ada` is still first after the update. Printing, `keys()`,
`values()`, and `items()` all follow that order.

Reading a missing key with brackets is a trap, as an out-of-range list index
is:

```aura
def main():
    grades = {"ada": 72, "grace": 95}
    print(grades["kathy"])
```

```console
$ aura run src/main.au
error[AU4003]: dict key `kathy` was not present
 --> src/main.au:3:18
  |
3 |     print(grades["kathy"])
  |                  ^
  = note: Aura call chain (innermost first): main at 1:1
```

So the rule is the one from lists. Use `grades[key]` when the key must be
there and its absence would be a bug. Use `key in grades` to test first, or
`get(key)` when absence is ordinary. `get` returns the same `Lookup` enum that
list `get` does, with `Lookup.Found(value)` holding a copy of the value, and
for the same reason: a dictionary's value could itself be `None`, and
`Lookup.Missing` can't be confused with it. Unlike Python's `dict.get`, it
takes no default argument; the `Missing` arm is where the default goes.

`remove(key)` takes the entry out and gives you its value, moved out of the
dictionary, inside a `Lookup.Found`. If the key wasn't there you get
`Lookup.Missing`, not a trap.

Dictionary keys can be any type that supports equality: numbers, strings,
tuples such as `("ada", 3)` for a student and an assignment number, and your
own classes. Lookups, insertions, and `in` take constant time on average,
however big the dictionary grows.

## Looping over a dictionary

You can't loop over a dictionary directly:

```aura check-fail:AU2999
def main():
    grades = {"ada": 72, "grace": 95}
    for name in grades:
        print(name)
```

```console
$ aura check src/main.au
error[AU2999]: `for` currently requires a `Range`, `Queue[T]`, `list[T]`, or `set[T]` iterable, found `dict[str, int64]`
 --> src/main.au:3:5
  |
3 |     for name in grades:
  |     ^
```

In Python, `for name in grades` loops over the keys, and it's a common
surprise for newcomers that it doesn't give the values. Aura asks you to say
which you want, using one of three methods that each return a list:

```aura
def main():
    grades = {"ada": 72, "grace": 95}
    for name, grade in grades.items():
        print(f"{name}: {grade}")
    for name in grades.keys():
        print(name)
    print(grades.values())
    print(sorted(grades.keys()))
```

```console
$ aura run src/main.au
ada: 72
grace: 95
ada
grace
[72, 95]
[ada, grace]
```

`items()` gives a list of `(key, value)` tuples, which a `for` loop can
unpack. These lists are *snapshots*: new lists holding copies of the keys and
values, taken when you call the method. Changing the dictionary afterwards
doesn't change a snapshot, and changing the snapshot doesn't touch the
dictionary. That makes them safe to loop over even while you update the
dictionary, which Python forbids with a runtime error. The price is the copy,
so for a dictionary of large values, prefer reading entries by key. The last
line uses `sorted`, which we'll meet properly in the [next
section](/book/ch08-04-comprehensions-and-builtins), to print the names in
alphabetical order instead of insertion order.

## Grouping scores by student

Now let's do something real. Our gradebook arrives as lines with a student
and one score each, in no particular order, and we want each student's
scores together, plus each student's best score. A `dict[str, list[int64]]`
holds the first, and a `dict[str, int64]` the second:

*Filename: src/main.au*

```aura check-fail:AU3001
def main():
    lines = ["ada 72", "grace 95", "ada 88", "alan 64", "grace 91"]
    mut scores: dict[str, list[int64]] = {}
    mut best: dict[str, int64] = {}
    for line in lines:
        pair = line.split_once(" ")
        if pair is not None:
            name, text = pair
            match parse_int64(text):
                case Result.Ok(score):
                    match best.get(name):
                        case Lookup.Found(current):
                            if score > current:
                                best[name] = score
                        case Lookup.Missing:
                            best[name] = score
                    if name in scores:
                        scores[name].append(score)
                    else:
                        scores[name] = [score]
                case Result.Err(message):
                    print(f"bad score for {name}: {message}")
    print(scores)
    print(best)
```

<span class="caption">Listing 8-13: Grouping scores by name (this won't compile yet)</span>

```console
$ aura check src/main.au
error[AU3001]: use of moved value `name`
 --> src/main.au:17:24
   |
17 |                     if name in scores:
   |                        ^
  = related src/main.au:14:38: value moved here
  = help: pass shared access when ownership is not needed, or call `.clone()` at the move site when an independent value is required
  = fix: replace src/main.au:14:42-14:42 with `.clone()`
```

The compiler says `name` was moved on line 14, by `best[name] = score`. It
looks odd that *storing a score* would move the name.

Think about what the dictionary needs. When `name` isn't already a key,
`best[name] = score` adds a new entry, and the dictionary has to own that
entry's key from now on, for as long as the entry exists. It can't borrow
the key from our loop, because `name` is gone by the next iteration. So
assigning through an index *moves the key in*, along with the value. The
compiler applies that rule to every indexed assignment on a dictionary, even
when the key happens to be present already, so `best[name] += 1` would move
`name` too. The same goes for line 16, the `Missing` arm: on either path,
`name` has been handed to `best` by the time we reach line 17.

Lookups are different. `best.get(name)`, `name in scores`, and `scores[name]`
on the right-hand side only need to compare `name` with the keys, so they
borrow it and give it back. That's why line 11 was fine.

There are two fixes. One is to put the moves last, so nothing needs `name`
afterwards. The other, which keeps our code in the order we'd describe it, is
to give `best` its own copy of the name, with `.clone()`, as the `fix` line
suggests. The `fix` only points at line 14, the first move the compiler
found, but both arms of the inner `match` store the name, so both need it:

```aura fragment
                    match best.get(name):
                        case Lookup.Found(current):
                            if score > current:
                                best[name.clone()] = score
                        case Lookup.Missing:
                            best[name.clone()] = score
```

```console
$ aura run src/main.au
{ada: [72, 88], grace: [95, 91], alan: [64]}
{ada: 88, grace: 95, alan: 64}
```

Now look at the second half of the loop, which never caused trouble:

```aura fragment
                    if name in scores:
                        scores[name].append(score)
                    else:
                        scores[name] = [score]
```

`scores[name].append(score)` changes the list stored under `name` in place,
without moving `name` and without copying the list out. The `else` arm moves
`name` into the dictionary as a new key, and since nothing uses `name` after
it, that's fine.

### Looking up and changing in one step

The `if name in scores` version looks the key up twice: once for `in` and
again for the index. For a value you want to change, the `lookup` method from
lists does it in one step, and works the same way on dictionaries:

```aura
def record(scores: mut dict[str, list[int64]], name: own str, score: int64):
    match mut scores.lookup(name):
        case Lookup.Found(existing):
            existing.append(score)
        case Lookup.Missing:
            scores[name] = [score]

def main():
    mut scores: dict[str, list[int64]] = {}
    record(scores, "ada", 72)
    record(scores, "ada", 88)
    record(scores, "grace", 95)
    print(scores)
```

```console
$ aura run src/main.au
{ada: [72, 88], grace: [95]}
```

`lookup` is only allowed as the subject of a `match`. When the key is
present, the `Found` arm runs with `existing` naming the list inside the
dictionary, and `match mut` lets the arm change it. When the key is missing,
the `Missing` arm runs and is free to insert. The parameter types tell the
same story as the loop did: `scores` is `mut` because `record` changes it, and
`name` is `own` because `record` may store it as a new key.

Two more methods round out dictionaries. `update(other)` moves every entry of
another dictionary into this one, replacing values for keys that already
exist, and `copy()` makes an independent copy of the whole dictionary. Note
that `update` consumes `other`: after `grades.update(more)`, `more` has been
moved. Two dictionaries are equal with `==` when they hold the same keys with
equal values, whatever order they were inserted in.

## Sets

A `set[T]` holds values with no duplicates, and its main job is answering
"is this in here?" quickly. Let's find the students who scored below 70 on
anything:

*Filename: src/main.au*

```aura
def main():
    scores = {"ada": [72, 88], "grace": [95, 91], "alan": [64, 58]}
    mut struggling = set[str]()
    for name, list_of_scores in scores.items():
        for score in list_of_scores:
            if score < 70:
                struggling.add(name.clone())
    print(struggling)
    print(struggling.len())
    print("alan" in struggling)
    print("ada" not in struggling)
    struggling.discard("ada")
    struggling.remove("alan")
    print(struggling)
```

<span class="caption">Listing 8-14: Collecting names in a set</span>

```console
$ aura run src/main.au
{alan}
1
true
true
set()
```

Alan has two low scores, so `add` is called for him twice, and the set still
holds him once. That's the whole point of a set: you can add freely without
first checking for a duplicate. We call `name.clone()` because the set needs
to own each name it holds, and `name` belongs to the snapshot list that
`items()` returned.

`in` and `not in` test membership, in constant time on average, which is the
reason to choose a set over a list when membership is the question. To take
a value out, use `remove`, which traps with `AU4008` if the value isn't there,
or `discard`, which does nothing in that case. An empty set prints as
`set()`, because `{}` would read as an empty dictionary.

That last point applies to literals too. A brace literal with colons is a
dictionary and one without is a set, so `{1, 2, 3}` is a `set[int64]`. An
empty `{}` is a dictionary, unless the binding's type says `set[...]`:

```aura
def main():
    sections = {1, 2, 2, 3}
    mut empty: set[str] = {}
    empty.add("math")
    print(sections)
    print(empty)
```

```console
$ aura run src/main.au
{1, 2, 3}
{math}
```

A set has no positions, so it has no indexing and no `get`. You can loop over
one with a bare `for` or consume it with `for value in own`, but not with
`for value in mut`. Changing a value in place could make it equal to another
value already in the set, and the set would no longer hold each value once.
Sets currently iterate in the order values were added, but that order isn't
part of the contract, so if you need a particular order, sort:
`sorted(struggling)` gives you a sorted list.

You may be used to Python's set operators, `a | b` for union and `a & b` for
intersection. Aura doesn't have them yet; `|` on two sets is an `AU2003`
error, because the compiler only knows `|` as a bitwise operator on integers.
For now, a comprehension or a short loop does the job, and comprehensions are
where we're going next.

## Choosing a collection

With three collection types, the choice usually comes down to the question
you'll ask most. If order matters, or duplicates do, use a list. If you look
things up by a key, use a dictionary. If you only need to know whether
something is present, use a set. When a program needs more than one view of
the same data, as our gradebook does with `scores` and `best`, it's normal to
keep more than one collection, each owning its own copy of the keys.

In the next section we'll stop writing so many loops. Comprehensions build a
list, a set, or a dictionary from another collection in one expression, and a
handful of builtins compute sums, minimums, and sorted copies without a loop
at all.
