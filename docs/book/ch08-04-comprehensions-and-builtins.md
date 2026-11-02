# Comprehensions and builtins

By now our gradebook code has a lot of loops that look the same: make an
empty collection, walk another one, maybe test something, append. Python
programmers write most of those as comprehensions, and Aura has the same
syntax for lists, sets, and dictionaries. It also has the builtins you'd
reach for next, such as `sum`, `max`, and `sorted`, so that a question like
"what's the best score in the class?" takes one line instead of five.

## List, set, and dictionary comprehensions

*Filename: src/main.au*

```aura
def main():
    scores = [72, 95, 64, 88, 91]
    curved = [score + 5 for score in scores]
    passing = [score for score in scores if score >= 70]
    bands: set[int64] = {score // 10 * 10 for score in scores}
    print(curved)
    print(passing)
    print(bands)
```

<span class="caption">Listing 8-15: A list comprehension, a filtered one, and a set comprehension</span>

```console
$ aura run src/main.au
[77, 100, 69, 93, 96]
[72, 95, 88, 91]
{70, 90, 60, 80}
```

A comprehension reads the way it does in Python. The expression at the front
is what goes into the new collection, the `for` clause says where the values
come from, and an optional `if` keeps only some of them. Square brackets make
a list and braces make a set, which here collapses the five scores into the
four distinct grade bands they fall into.

Each comprehension runs as soon as it's evaluated and produces a complete,
owned collection. There's nothing lazy about it. The source is read with
shared access, as in a bare `for` loop, so `scores` is unchanged afterwards
and can't be changed while the comprehension is running.

The loop name, `score` here, belongs to the comprehension alone. It doesn't
exist after the closing bracket, and it can't reuse a name that's already in
use around it:

```aura check-fail:AU2999
def main():
    score = 100
    scores = [72, 95]
    curved = [score + 5 for score in scores]
```

```console
$ aura check src/main.au
error[AU2999]: comprehension binding `score` would shadow an existing name
 --> src/main.au:4:29
  |
4 |     curved = [score + 5 for score in scores]
  |                             ^
```

In Python 3 the comprehension's `score` would quietly hide the outer one
inside the brackets, and `score + 5` would mean the loop value. That's
probably what you meant, but a reader has to know the scoping rule to be
sure. Aura applies the same no-shadowing rule it uses for `for` loops, so one
spelling means one thing throughout a function.

You can have more than one `for` clause, and the clauses nest the same way
written-out loops would, with the first clause outermost:

```aura
def main():
    names = ["ada", "grace", "alan"]
    assignments = [1, 2]
    slots = [f"{name}-{number}" for name in names for number in assignments if name != "alan"]
    print(slots)
```

```console
$ aura run src/main.au
[ada-1, ada-2, grace-1, grace-2]
```

A dictionary comprehension uses braces with a `key: value` pair at the
front. Let's map each name to its length:

```aura check-fail:AU3002
def main():
    names = ["ada", "grace", "alan"]
    lengths = {name: name.len() for name in names}
```

```console
$ aura check src/main.au
error[AU3002]: cannot move borrowed value `name`
 --> src/main.au:3:16
  |
3 |     lengths = {name: name.len() for name in names}
  |                ^
  = related src/main.au:3:37: value is borrowed here
  = help: comprehensions store owned values; call `.clone()` on this shared value, or use an explicit consuming loop when the source should be transferred
  = fix: replace src/main.au:3:20-3:20 with `.clone()`
```

This is the same situation as the dictionary keys in the last section, seen
from another angle. The comprehension walks `names` with shared access, so
each `name` is borrowed from the list. The new dictionary has to own its keys,
and it can't own a string that the list still owns. The numbers in the first
listing didn't have this problem because they're copy values. The help line
gives both ways out: clone each name, or, if you're done with `names`, write
a `for name in own names` loop that moves each string into the dictionary
without copying.

```aura
def main():
    names = ["ada", "grace", "alan"]
    lengths = {name.clone(): name.len() for name in names}
    print(lengths)
```

```console
$ aura run src/main.au
{ada: 3, grace: 5, alan: 4}
```

`name.len()` needed no clone, because it produces a new number rather than
storing the name. The rule to remember is that whatever ends up *in* the new
collection must be owned by it: copy values copy, freshly computed values
such as `name.len()` or `name.to_upper()` are already owned, and owned values
borrowed from the source need `.clone()`.

Comprehensions only take the bare form of `for`. Writing `for name in mut
names` or `for name in own names` inside the brackets is a syntax error
(`AU1101`), and the message tells you to use a statement loop for those.

### Something comprehensions don't do

Python lets you drop the brackets inside a function call, as in `sum(score *
2 for score in scores)`, to make a *generator expression* that produces
values one at a time. Aura has no generators:

```aura check-fail:AU2005
def main():
    scores = [72, 95]
    print(sum(s * 2 for s in scores))
```

```console
$ aura check src/main.au
error[AU2005]: generator expressions are unavailable; use an eager owned list comprehension or an explicit loop
 --> src/main.au:3:21
  |
3 |     print(sum(s * 2 for s in scores))
  |                     ^
```

Add the brackets, `sum([s * 2 for s in scores])`, and you get a list
comprehension, which builds the whole list first. For the collection sizes in
most programs that's fine.

## Reducing a collection to one value

Five builtins take a collection and boil it down to a single answer:

*Filename: src/main.au*

```aura
def main():
    scores = [72, 95, 64, 88, 91]
    print(sum(scores))
    print(min(scores))
    print(max(scores))
    print(any([score < 65 for score in scores]))
    print(all([score >= 60 for score in scores]))
    print(sum(range(1, 11)))
    print(max(72, 95, 64))
    average = sum(scores).to_float() / len(scores).to_float()
    print(f"average {average:.1f}")
```

<span class="caption">Listing 8-16: `sum`, `min`, `max`, `any`, and `all`</span>

```console
$ aura run src/main.au
410
64
95
true
true
55
95
average 82.0
```

`sum`, `min`, and `max` work on a list, a set, or a range of numbers, and
return the element type: these scores are `int64`, so the sum is an `int64`.
`any` and `all` take a collection of `bool`s, which a comprehension is good
at producing; `any` stops at the first `true` and `all` at the first `false`.
`min` and `max` also accept two or more separate numbers of the same type, as
in `max(72, 95, 64)`, and they work on strings too: `min("pear", "apple",
"fig")` is `"apple"`, and the result is a copy you own. All of them read the
collection with shared access and leave it as it was.

The average shows a step you'll write often. `sum(scores)` and `len(scores)`
are both integers, and Aura doesn't do integer `/`, so we convert each to a
`float64` with `.to_float()` before dividing. Python's `/` silently turns two
integers into a float; Aura makes you say so, for the reasons we saw in
[Chapter 3](/book/ch03-00-common-concepts).

Each builtin has a defined answer for an empty collection where one makes
sense. `sum` of nothing is zero, `any` of nothing is `false`, and `all` of
nothing is `true`. There's no sensible minimum or maximum of nothing, so
`min` and `max` trap:

```aura
def main():
    empty: list[int64] = []
    print(max(empty))
```

```console
$ aura run src/main.au
error[AU4001]: `max(...)` of an empty collection
 --> src/main.au:3:11
  |
3 |     print(max(empty))
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

Python raises a `ValueError` here. When a list may be empty, check
`values.is_empty()` before asking for its maximum.

One more note on `sum`: it uses the same checked `+` as the rest of the
language, so adding up `int64` values that overflow stops with `AU4002`
instead of wrapping around to a negative number.

## Building new lists

Another group of builtins and operators make a new list from existing ones:

*Filename: src/main.au*

```aura
def main():
    scores = [72, 95, 64, 88, 91]
    print(sorted(scores))
    print(sorted(scores, reverse=true))
    print(reversed(scores))
    print(scores[::-1])
    print(scores[::2])
    print(scores + [100, 59])
    print([0] * 3)
    print(list(range(1, 6)))
    print(scores)
```

<span class="caption">Listing 8-17: Making sorted, reversed, joined, and repeated lists</span>

```console
$ aura run src/main.au
[64, 72, 88, 91, 95]
[95, 91, 88, 72, 64]
[91, 88, 64, 95, 72]
[91, 88, 64, 95, 72]
[72, 64, 91]
[72, 95, 64, 88, 91, 100, 59]
[0, 0, 0]
[1, 2, 3, 4, 5]
[72, 95, 64, 88, 91]
```

The last line is the important one: none of these touched `scores`. Each
one copies elements into a fresh list. That's the difference between
`sorted(scores)` and `scores.sort()`, and between `reversed(scores)` and
`scores.reverse()`: the builtin gives you a new list and works on a value you
only have shared access to, while the method rearranges a list you can
change. `sorted` takes the same keyword arguments as `sort`, `key=` and
`reverse=`, and is stable in the same way. Unlike Python's `reversed`, which
returns a lazy iterator, Aura's returns a list.

A slice with a third number takes a step: `scores[::2]` keeps every second
element, and `scores[::-1]` walks backwards from the end, which reverses the
list. Steps work on lists and on strings. A step of zero traps, and so does
a negative step whose start and end don't make sense going backwards. Aura
never clamps a slice to fit, for steps or otherwise.

`+` joins two lists of the same element type into a new list, and `*`
repeats one. `list(range(1, 6))` turns a range into a real list, and
`list(some_set)` turns a set into one. When you want to *grow* a list rather
than build a new one, `scores += [100, 59]` on a mutable list adds the
elements in place, without copying the existing ones. That makes `+=` the
right choice inside a loop, where `scores = scores + more` would copy the
whole list on every step.

There are a few limits to know about. `sorted`, `reversed`, and `list` don't
take a `str` or a dictionary; to sort a dictionary's keys, write
`sorted(table.keys())`, as we did in the last section. And because each of
these copies elements, the element type has to be one that can be copied:
copy values, strings, lists, and classes made of those are all fine.

The names `sum`, `min`, `max`, `any`, `all`, `sorted`, `reversed`, `list`,
`ord`, and `chr` belong to the builtins, so a top-level function can't use them:
`def sum(...)` is rejected with `AU2007`. A local binding can still have one
of those names, although giving a binding the same name as a builtin is
rarely a kindness to the reader.

## Putting it together

Let's put this section to work on the whole gradebook: rank the students by
average, find who needs help, and report the best single score.

*Filename: src/main.au*

```aura
def average(values: list[int64]) -> float64:
    return sum(values).to_float() / len(values).to_float()

def main():
    scores = {
        "ada": [72, 88, 95],
        "grace": [95, 91, 84],
        "alan": [64, 58, 77],
        "kathy": [81, 79, 90],
    }
    averages = [(name.clone(), average(marks)) for name, marks in scores.items()]
    ranking = sorted(averages, key=lambda entry: entry[1], reverse=true)
    for name, mean in ranking:
        print(f"{name:<6} {mean:.1f}")

    struggling = {name.clone() for name, marks in scores.items() if min(marks) < 70}
    print(f"needs help: {', '.join(sorted(struggling))}")

    best_marks = [max(marks) for marks in scores.values()]
    print(f"best single score: {max(best_marks)}")
    print(f"everyone has a pass: {all([mark >= 70 for mark in best_marks])}")
```

<span class="caption">Listing 8-18: Ranking, filtering, and summarizing the gradebook</span>

```console
$ aura run src/main.au
grace  90.0
ada    85.0
kathy  83.3
alan   66.3
needs help: alan
best single score: 95
everyone has a pass: true
```

The first comprehension builds a list of `(name, average)` tuples, one per
student, cloning each name because the new list has to own it. `sorted` then
orders those tuples by their second element, highest first, and the `for`
loop unpacks each tuple back into a name and an average.

You might have expected a dictionary from name to average here. It would be
natural, and `sorted(scores.keys(), key=lambda name: averages[name])` is what
you'd write in Python. In Aura that lambda would *take ownership* of
`averages`, because a lambda that uses a local value captures it by moving it
in, and then the `print` loop couldn't read `averages` any more. The compiler
says exactly that ("value moved into closure here") if you try.
[Chapter 13](/book/ch13-00-closures-and-decorators) explains closures and
captures properly. Sorting a list of tuples sidesteps the question, because
the lambda only uses its own parameter.

The rest is the builtins at work: a set comprehension with a filter for the
students who scored below 70 at least once, `sorted` to print them in a
predictable order, and `max` used twice, once inside a comprehension for each
student's best and once over those bests.

Everything in this section has worked on general-purpose collections, where
each element can be any type. For tables of numbers there's one more type,
which trades that flexibility for speed and arithmetic on a whole table at
once.
