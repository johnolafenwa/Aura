# Defining and building classes

A class declaration lists the fields every instance has, with a type for each.
Here is a class for a book:

```aura
class Book:
    title: str
    author: str
    pages: int64
```

<span class="caption">Listing 5-1: A `Book` class with three fields</span>

The declaration starts with `class` and the class name, by convention in
`CamelCase`. The indented body holds one line per field: a name, a colon, and
a type. The order of the fields matters in one place, positional construction,
which we'll get to in a moment.

Compare this with a Python class, where fields usually appear for the first
time as assignments inside `__init__`. A Python object can grow new
attributes at any point, and nothing stops two `Book` objects from having
different sets of them. An Aura class is a fixed shape. Every `Book` has
exactly these three fields with exactly these types, and the compiler checks
every construction and every field access against that shape.

## Creating instances

To make a `Book`, call the class name with a value for each field:

*Filename: src/main.au*

```aura
class Book:
    title: str
    author: str
    pages: int64

def main():
    book = Book(title="Dune", author="Frank Herbert", pages=412)
    print(book.title)
    print(book.pages)
    print(book)
    other = Book("Emma", "Jane Austen", pages=474)
    print(other.author)
```

<span class="caption">Listing 5-2: Building two books and reading their fields</span>

```console
$ aura run src/main.au
Dune
412
Book(author=Frank Herbert, pages=412, title=Dune)
Jane Austen
```

The first call names every field. The second passes `title` and `author` by
position, in the order the class declares them, and then names `pages`. You
can mix the two styles as long as the positional arguments come first. Named
arguments are easier to read when a class has more than two or three fields,
and they keep working if someone reorders the declaration later, so we'll use
them most of the time.

You read a field with a dot, as in `book.title`. Printing a whole instance
shows the class name and each field with its value. The fields appear in
alphabetical order, not declaration order. That rendering is meant for
debugging; in [Methods](/book/ch05-03-methods) we'll see how to give a class
its own.

The compiler checks each construction against the declaration. Leave a field
out and it names the one that's missing:

```aura check-fail:AU2999
class Book:
    title: str
    author: str
    pages: int64

def main():
    book = Book(title="Dune", author="Frank Herbert")
```

```console
$ aura check src/main.au
error[AU2999]: class constructor `Book` is missing required field `pages`
 --> src/main.au:7:12
  |
7 |     book = Book(title="Dune", author="Frank Herbert")
  |            ^
```

And a field the class doesn't have is an error too, pointing at the argument
that doesn't belong:

```aura check-fail:AU2999
class Book:
    title: str
    author: str
    pages: int64

def main():
    book = Book(title="Dune", author="Frank Herbert", pages=412, year=1965)
```

```console
$ aura check src/main.au
error[AU2999]: class `Book` has no field named `year`
 --> src/main.au:7:66
  |
7 |     book = Book(title="Dune", author="Frank Herbert", pages=412, year=1965)
  |                                                                  ^
```

In Python both mistakes would show up later, if at all: a missing attribute
as an `AttributeError` the first time some code reads it, and a misspelled
one as a new attribute that nothing ever reads. Aura reports them at the
line that built the instance.

## Default values

A field can have a default, written after its type. Fields with defaults can
be left out of the constructor call:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    tags: list[str] = []

def main():
    mut dune = Book(title="Dune", pages=412)
    emma = Book(title="Emma", pages=474)
    dune.tags.append("science fiction")
    print(dune.tags)
    print(emma.tags)
```

<span class="caption">Listing 5-3: Each instance gets its own fresh default list</span>

```console
$ aura run src/main.au
[science fiction]
[]
```

If you've been bitten by Python's mutable default arguments, look at the
output again. Aura evaluates a default expression afresh for every
construction that leaves the field out, so `dune` and `emma` each get their
own empty list, and appending to one leaves the other alone. Under ownership
there is no other sensible choice: one list can't have two owners, so two
books can't share a default list.

## Changing fields

Assigning to a field changes the instance, and the instance has to be in a
mutable binding for that, the same way a list does before you can append to it:

```aura check-fail:AU3003
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

def main():
    book = Book(title="Dune", pages=412)
    book.pages_read = 50
```

```console
$ aura check src/main.au
error[AU3003]: cannot assign through immutable place `book.pages_read`
 --> src/main.au:8:5
  |
8 |     book.pages_read = 50
  |     ^
```

Mutability belongs to the binding, and the fields inherit it. A `mut` binding
lets you change any field, and an immutable one lets you change none. There
is no way to mark one field read-only and another writable. Declaring the
binding `mut` fixes the error:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

def main():
    mut book = Book(title="Dune", pages=412)
    book.pages_read = 50
    book.pages_read += 25
    print(book.pages_read)
```

```console
$ aura run src/main.au
75
```

## Classes are owned values

A class instance is an owned value, so everything from
[Chapter 4](/book/ch04-00-understanding-ownership) applies to it. Assigning
it moves it. Passing it to an `own` parameter moves it. Putting it into a list
moves it too:

```aura check-fail:AU3001
class Book:
    title: str
    pages: int64

def main():
    book = Book(title="Dune", pages=412)
    shelf = [book]
    print(book.title)
```

```console
$ aura check src/main.au
error[AU3001]: use of moved value `book`
 --> src/main.au:8:11
  |
8 |     print(book.title)
  |           ^
  = related src/main.au:7:14: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

The list literal `[book]` on line 7 took ownership of the book. From then on
the book lives in `shelf`, and we'd reach it as `shelf[0]`. The construction
call itself works the same way: every field argument is an owned position, so
`Book(title=name, pages=412)` moves the string in `name` into the new book.

### Copy classes

Some classes are as small and simple as a number. For those you can write
`copy class`, and the instances copy on assignment the way integers do:

*Filename: src/main.au*

```aura
copy class Rating:
    stars: int64
    out_of: int64

def main():
    first = Rating(stars=4, out_of=5)
    mut second = first
    second.stars = 5
    print(first.stars)
    print(second.stars)
```

<span class="caption">Listing 5-4: A copy class duplicates on assignment</span>

```console
$ aura run src/main.au
4
5
```

`second = first` made a copy, so changing `second` left `first` at four
stars. A copy class may only contain copy values. Give `Rating` a `str` field
and the compiler stops you, because a string can't be duplicated for free:

```aura check-fail:AU2002
copy class Rating:
    stars: int64
    comment: str
```

```console
$ aura check src/main.au
error[AU2002]: field `comment` on `copy class Rating` must be a copy type, found `str`
 --> src/main.au:3:5
  |
3 |     comment: str
  |     ^
```

Use `copy class` when copying is part of what the type means: a point, a
color, a rating, a pair of numbers. Don't use it to get out of an ownership
error. If a class holds strings or lists, it is an owned value, and the
ownership rules are doing their job.

## Initializers

Sometimes the constructor shouldn't take the fields one for one. Perhaps a
field is always computed, or an argument needs checking first. For those
cases a class can declare `__init__`, as in Python:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64

    def __init__(self, title: own str, pages: int64):
        self.title = title
        if pages < 1:
            self.pages = 1
        else:
            self.pages = pages
        self.pages_read = 0

def main():
    book = Book("Dune", 412)
    blank = Book("Notebook", 0)
    print(book)
    print(blank.pages)
```

<span class="caption">Listing 5-5: An initializer that computes and checks fields</span>

```console
$ aura run src/main.au
Book(pages=412, pages_read=0, title=Dune)
1
```

Once a class has `__init__`, calling the class calls the initializer. Its
parameters replace the field list, so `Book("Dune", 412)` passes a title and a
page count, and `Book(title="Dune", pages=412, pages_read=10)` is now an
error that says the class expects two arguments. Inside the body, `self` is
the instance being built, and the job of the body is to give every field a
value.

The compiler checks that it does, on every path. Delete the last line,
`self.pages_read = 0`, and you get:

```console
$ aura check src/main.au
error[AU3012]: initializer falls through without initializing `pages_read`
 --> src/main.au:6:5
  |
6 |     def __init__(self, title: own str, pages: int64):
  |     ^
  = help: assign every field without a declaration default on every path through `__init__`
```

The `if` sets `pages` on both branches, so that field is fine. `pages_read`
is set on no path at all. Python would let this through, and the first read
of `book.pages_read` would raise an `AttributeError` far from the cause. In
Aura, an instance with an unset field can't exist.

Notice the `own` on `title`. The initializer stores the string in a field,
which keeps it, so it needs ownership. If we write `title: str` instead, the
compiler catches the assignment that tries to keep a borrowed value:

```console
$ aura check src/main.au
error[AU3002]: parameter `title` is borrowed; declare it as `own str` to take ownership, or clone the value before consuming it
 --> src/main.au:7:22
  |
7 |         self.title = title
  |                      ^
  = related src/main.au:6:24: parameter `title` is borrowed here
  = help: declare the parameter as `own str` when the function should consume it, or call `.clone()` to consume an independent copy
  = fix: replace src/main.au:7:27-7:27 with `.clone()`
```

That is the "pick the weakest access that does the job" rule from the last
chapter, applied to a constructor. Storing is keeping, and keeping needs
`own`.

An initializer has no return value and can't fail. When building an
instance can go wrong, for example when a page count comes from user input,
write a function on the class that returns a `Result` instead. We'll see
functions on the class in [Methods](/book/ch05-03-methods) and `Result` in
[Chapter 9](/book/ch09-00-error-handling). The
[Manual](/manual/classes) has the complete construction rules, including
evaluation order and visibility across modules.

With classes in hand, let's put them to work in a small program and see what
they change.
