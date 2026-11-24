# Views and spans

Every collection in this chapter owns its elements, and that has made one
ordinary request awkward: giving a name to something that lives inside a
collection. Copy values such as `int64` come out as copies, so `x = scores[0]`
works. A class instance or a string does not copy, and the gradebook is full
of both. Let's ask for the student at the top of the list:

*Filename: src/main.au*

```aura check-fail:AU3005
class Student:
    name: str
    points: int64

def main():
    mut students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    leader = students[1]
    print(leader.name)
```

```console
$ aura check src/main.au
error[AU3005]: cannot implicitly copy `Student` out of a list index; use `view value = items[index]` for shared access, or `pop(index)` or `set(index, value)` to transfer ownership
 --> src/main.au:7:14
  |
7 |     leader = students[1]
  |              ^
```

An ordinary binding owns its value. If `leader` owned this student, either
the list would have to give her up, leaving a hole at index 1, or there would
be two owners of one `Student`. Aura allows neither, and the message lists
the ways out. `pop` and `set` move the element out of the list. The first
suggestion is the one we want here: we do not want to take the student
anywhere, we want to look at her where she is.

## Naming a place with `view`

A *view* is a name for a place that something else owns. It is created with
the `view` keyword in front of a binding, and it gives shared access to the
place on the right:

*Filename: src/main.au*

```aura
class Student:
    name: str
    points: int64

def main():
    mut students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    view leader = students[1]
    print(f"{leader.name} leads with {leader.points}")

    view mut trailing = students[0]
    trailing.points += 20
    trailing.name = "Ada L."
    print(f"{students[0].name} now has {students[0].points}")
```

<span class="caption">Listing 8-27: A shared view and a mutable view of list elements</span>

```console
$ aura run src/main.au
Grace leads with 85
Ada L. now has 90
```

`leader` is not a copy of the second student. It is that student, under
another name, for reading. `view mut` asks for mutable access instead, so
assignments through `trailing` land in the list, which is why the last line
sees the new name and the new total when it reads `students[0]` directly. A
mutable view needs a mutable place to point at, for the same reason a `mut`
parameter needs a `mut` argument: `students` is declared with `mut`.

Making a view costs nothing at run time. No element is copied and nothing is
allocated. The index is evaluated once, when the view is created, and an
index past the end traps there with `AU4003` like any other index. After
that the view keeps naming the same slot.

You have been using views without the keyword since
[Chapter 4](/book/ch04-03-copies-and-slices). When you write
`print(students[1].name)` or pass `students[1]` to a function, Aura lends the
element for the length of that statement. The `view` keyword is for when the
loan needs a name and has to last for more than one statement.

## What a view holds on to

A view is only safe if the place it names stays put. A list keeps its
elements in one block of memory, and `append` may move that block somewhere
larger. So what happens if we grow the list while a view is looking into it?

```aura check-fail:AU3011
class Student:
    name: str
    points: int64

def main():
    mut students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    view leader = students[1]
    students.append(Student(name="Margaret", points=90))
    print(leader.name)
```

```console
$ aura check src/main.au
error[AU3011]: cannot call `append` on `students` while shared element view `leader` of `students[1]` remains live
 --> src/main.au:8:14
  |
8 |     students.append(Student(name="Margaret", points=90))
  |              ^
  = related src/main.au:7:5: view `leader` starts here
  = related src/main.au:9:11: last use of view `leader` keeps this loan live
```

The diagnostic names three places. The `append` is the operation that was
refused. The first `related` line is where the view began, and the second is
the reason it still matters: `leader` is used again on line 9, after the
list would have changed. In C++ this program compiles, and the last line
reads through a pointer into memory the vector may already have freed. Aura
refuses to build it.

Read the last `related` line again, because it also tells us how to fix the
program. A view is live from the line that creates it to its *last use*, not
to the end of the block. If we finish with `leader` before we grow the list,
there is nothing left to protect:

```aura
class Student:
    name: str
    points: int64

def main():
    mut students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    view leader = students[1]
    print(leader.name)
    students.append(Student(name="Margaret", points=90))
    print(students.len())
```

```console
$ aura run src/main.au
Grace
3
```

While an element view is live, the list cannot change shape: no `append`,
`insert`, `pop`, `remove`, `clear`, `sort`, `reverse`, or `swap`. A
dictionary is stricter still. Any assignment into it is refused, because a
new key can move every entry.

Two views into one collection follow the rule you know from
[Chapter 4](/book/ch04-02-shared-and-mutable-access): any number of shared
views, or one mutable view. There is one useful exception. When both indexes
are written as literals and they differ, the compiler can see that the two
views name different elements, and it allows two mutable views side by side:

```aura
class Student:
    name: str
    points: int64

def main():
    mut students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    view mut first = students[0]
    view mut second = students[1]
    first.points += 1
    second.points += 2
    print(students[0].points + students[1].points)
```

```console
$ aura run src/main.au
158
```

Replace the literals with bindings `i` and `j` and the second view is
refused with `AU3002`, even if `i` and `j` happen to hold different numbers.
The checker reasons about the program text, and it cannot know what a
binding will hold when the program runs. When you need two elements chosen at
run time, finish with one view before you take the next, or use `swap`.

Lists are not the only things with places. A view can name a field, a
dictionary entry, or another view:

```aura
def main():
    mut absences = {"Ada": 1, "Grace": 0}
    view mut count = absences["Ada"]
    count += 2
    print(absences)
```

```console
$ aura run src/main.au
{Ada: 3, Grace: 0}
```

A key that is not in the dictionary traps when the view is created, as
indexing with it would.

## Returning a view

Now let's move the search for the leader into a function. The function has
to hand back a student that lives in the caller's list. The first signature
most of us would write says that it returns a `Student`:

```aura check-fail:AU3005
class Student:
    name: str
    points: int64

def leader(students: list[Student]) -> Student:
    mut best = 0
    for i in range(students.len()):
        if students[i].points > students[best].points:
            best = i
    return students[best]

def main():
    students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    print(leader(students).name)
```

```console
$ aura check src/main.au
error[AU3005]: cannot implicitly copy `Student` out of a list index; use `view value = items[index]` for shared access, or `pop(index)` or `set(index, value)` to transfer ownership
 --> src/main.au:10:12
   |
10 |     return students[best]
   |            ^
```

It is the error from the top of this section, for the same reason. An
ordinary `-> Student` result is owned by the caller, and `leader` has only
shared access to the list, so it has no student to give away.

A function can return a view instead. The return type says three things:
that the result is a view, what type it views, and which parameter it lives
inside.

*Filename: src/main.au*

```aura
class Student:
    name: str
    points: int64

def leader(students: list[Student]) -> view Student from students:
    mut best = 0
    for i in range(students.len()):
        if students[i].points > students[best].points:
            best = i
    return view students[best]

def leader_mut(students: mut list[Student]) -> view mut Student from students:
    mut best = 0
    for i in range(students.len()):
        if students[i].points > students[best].points:
            best = i
    return view mut students[best]

def main():
    mut students = [Student(name="Ada", points=70), Student(name="Grace", points=85)]
    view top = leader(students)
    print(f"{top.name} leads with {top.points}")

    view mut winner = leader_mut(students)
    winner.points += 5
    print(students[1].points)
```

<span class="caption">Listing 8-28: Functions that return a shared and a mutable view</span>

```console
$ aura run src/main.au
Grace leads with 85
90
```

`-> view Student from students` reads as "a view of a `Student` that lives
in `students`". The `from` clause is the part with no counterpart in Python.
It tells the caller which of its own values the result points into, so that
the caller's checker can apply the rules from the last section without
looking inside `leader`. While `top` is live, `students` is on loan, and
appending to it is refused exactly as before. A `view mut` result needs a
`mut` parameter to come from, so `leader_mut` takes `mut list[Student]`.

The compiler checks the body against the promise. Every `return view` must
name the declared origin or something inside it. A view of a local would
outlive the local, so it is refused:

```aura check-fail:AU3010
def first(names: list[str]) -> view str from names:
    sorted_names = sorted(names)
    return view sorted_names[0]

def main():
    names = ["Grace", "Ada"]
    view earliest = first(names)
    print(earliest)
```

```console
$ aura check src/main.au
error[AU3010]: returned view derives from `sorted_names`, but the declaration names origin `names`
 --> src/main.au:3:17
  |
3 |     return view sorted_names[0]
  |                 ^
  = related src/main.au:1:32: returned-view origin is declared here
```

`sorted_names` is a new list that `first` owns, and it is dropped when
`first` returns. A function that wants to hand back something it built
returns it by value, with an ordinary `-> str`.

At the call site, a returned view has to be bound with `view` (or
`view mut`), or used on the spot as in `print(leader(students).name)`.
Writing `top = leader(students)` is the `AU3010` error again, with a help
line that says to write `view top = ...`.

Methods can return views of `self` the same way, which is how a class hands
out one of its fields without cloning it:

```aura
class Course:
    title: str
    students: list[str]

    def representative(self) -> view str from self:
        return view self.students[0]

def main():
    course = Course(title="Compilers", students=["Grace", "Ada"])
    view name = course.representative()
    print(name)
```

```console
$ aura run src/main.au
Grace
```

## Spans: a view of a range

A view names one place. Often we want a run of places: the last three
scores, the first half of a list. [Chapter 4](/book/ch04-03-copies-and-slices)
showed that a slice such as `scores[1:4]` builds a new list and copies the
elements into it. That is the right default, because the copy is independent
of the original. It is also wasted work when all we want is to read the
range, and it cannot help at all when we want to change the range in place.

Put `view` in front of the binding and the slice becomes a `Span[T]`: a view
of a contiguous range of a list.

*Filename: src/main.au*

```aura
def average(values: Span[float64]) -> float64:
    return sum(values) / (values.len() as float64)

def main():
    mut temps = [18.5, 19.0, 21.5, 24.0, 23.5, 20.0, 17.5]
    print(average(temps[1:4]))
    print(average(temps))

    view mut afternoon = temps[3:6]
    afternoon.sort()
    afternoon[0] = 0.0
    print(afternoon.len())
    print(afternoon.to_list())
    print(temps)
```

<span class="caption">Listing 8-29: Reading and changing a range of a list through spans</span>

```console
$ aura run src/main.au
21.5
20.571428571428573
3
[0.0, 23.5, 24.0]
[18.5, 19.0, 21.5, 0.0, 23.5, 24.0, 17.5]
```

`average` takes a `Span[float64]`. A span parameter accepts a slice, a whole
list, or another span, so `average(temps[1:4])` and `average(temps)` both
work and neither copies a float. Inside the function a span reads like a
list: it has `len()`, indexing, `for` loops, `in`, and the builtins `sum`,
`min`, `max`, and `sorted`.

`afternoon` is a mutable span of three elements. Positions in a span count
from the start of its range, so `afternoon[0]` is `temps[3]`. Sorting the
span sorts those three elements where they sit, and the final line shows the
change in `temps` with its neighbors untouched.

A span has a fixed length. It can reorder and overwrite its elements with
`sort`, `reverse`, `swap`, and `fill`, but it has no `append` or `pop`,
because those would change the length of a list it does not own. When you do
want an independent copy of the range, `to_list()` makes one. That is also
how you print a span: a span has no text form of its own, and `print(afternoon)`
is an error whose help line suggests `to_list()`.

### An in-place sort

Spans earn their keep in algorithms that work on smaller and smaller pieces
of one list. Quicksort is the classic one: pick a pivot, move everything
smaller to its left, then sort the left part and the right part the same
way. With copying slices, every level of the recursion would allocate new
lists. With spans, the whole sort happens inside the caller's list:

*Filename: src/main.au*

```aura
def partition(values: mut Span[int64]) -> int64:
    last = values.len() - 1
    pivot = values[last]
    mut boundary = 0
    for i in range(last):
        if values[i] < pivot:
            values.swap(i, boundary)
            boundary += 1
    values.swap(boundary, last)
    return boundary

def quicksort(values: mut Span[int64]):
    if values.len() < 2:
        return
    middle = partition(values)
    quicksort(values[:middle])
    quicksort(values[middle + 1:])

def main():
    mut readings = [31, 4, 15, 9, 26, 5, 35, 8]
    quicksort(readings)
    print(readings)

    mut partly = [9, 8, 7, 3, 2, 1]
    quicksort(partly[3:])
    print(partly)
```

<span class="caption">Listing 8-30: Quicksort over mutable spans</span>

```console
$ aura run src/main.au
[4, 5, 8, 9, 15, 26, 31, 35]
[9, 8, 7, 1, 2, 3]
```

`partition` moves the pivot into its final position and returns that
position. `quicksort` then slices the span it was given into the part before
the pivot and the part after it. Slicing a span gives a narrower span of the
same list, with positions counted from the start of the span being sliced,
so each recursive call sees a small list that starts at index 0 and has no
idea where it sits in `readings`. The second call in `main` sorts only the
back half of `partly`. You would not write a sort yourself in a real
program, since `list.sort` and `Span.sort` exist, but the same shape serves
binary search, merging, and any other divide-and-conquer work.

### What a span cannot do

A span is a view, so everything in "What a view holds on to" applies to it,
with one difference: a span borrows the whole list, not one element. While a
shared span is live the list can be read and nothing else. While a mutable
span is live the list can only be reached through the span.

Because it is a borrow, a span cannot be stored. It cannot be a class field,
an element of another collection, or an owned local, and it cannot be sent
to another task. A class that wants to remember a range keeps the two
indexes and makes the span when it needs one. A function can return a span
of a list parameter, declared like any other returned view:

```aura
def without_ends(values: list[int64]) -> view Span[int64] from values:
    return view values[1:-1]

def main():
    scores = [4, 9, 2, 7]
    view inner = without_ends(scores)
    print(inner.to_list())
```

```console
$ aura run src/main.au
[9, 2]
```

Spans cover `list[T]` only. There is no span of a `str` or of an `Array` in
Aura 0.17 (an `Array` slice already shares memory with its array, as the
last section showed), and a method cannot yet return a span of `self`. The Manual's pages on
[ownership](/manual/ownership-and-borrowing#local-views-and-place-identity) and
[collections](/manual/collections#span-t) have the complete rules.

Views let us read and change numbers in place. In the next section we use
that to do arithmetic on several of them at once.
