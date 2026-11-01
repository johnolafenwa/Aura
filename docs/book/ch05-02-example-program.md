# An example program using classes

To see what a class changes, let's write a small program the long way first.
We want a reading tracker: given how many pages a book has and how many we've
read, it tells us how far through we are. We'll start with separate bindings,
move to a tuple, and end with a class, looking at what each version makes
easy and what it leaves to chance.

Make a new project for it:

```console
$ aura new reading
created `reading`
```

All the listings in this section replace the contents of
`reading/src/main.au`, and we run them from inside the `reading` directory.

## Starting with loose bindings

The first version keeps the two numbers in their own bindings and passes both
to a function:

*Filename: src/main.au*

```aura
def percent_read(pages: int64, pages_read: int64) -> float64:
    return pages_read.to_float() / pages.to_float() * 100.0

def main():
    pages = 412
    pages_read = 103
    print(f"You have read {percent_read(pages, pages_read):.1f}% of the book.")
```

<span class="caption">Listing 5-6: Computing progress from two separate bindings</span>

```console
$ aura run src/main.au
You have read 25.0% of the book.
```

`percent_read` converts both counts to `float64` with `.to_float()`,
because Aura never converts between integer and float types for you, and `/`
isn't defined on two `int64` values at all (that was the `AU2003` error in
[Data types](/book/ch03-02-data-types)). A percentage is a ratio, so the
rounding `.to_float()` allows is fine here. The `:.1f` in the f-string prints
the result with one decimal place, as it would in Python.

The program works, but look at the signature: `percent_read(pages: int64,
pages_read: int64)`. Both parameters have the same type, so nothing stops us
from passing them in the wrong order. Swap them in the call and see:

```console
$ aura run src/main.au
You have read 400.0% of the book.
```

The compiler had no reason to object. As far as it knows, any two `int64`
values will do. The connection between `pages` and `pages_read` (that they
describe the same book, and which is which) exists only in our heads and in
the parameter names.

## Grouping with a tuple

A tuple lets us carry the two numbers as one value:

*Filename: src/main.au*

```aura
def percent_read(progress: (int64, int64)) -> float64:
    return progress[1].to_float() / progress[0].to_float() * 100.0

def main():
    dune = (412, 103)
    print(f"You have read {percent_read(dune):.1f}% of the book.")
```

<span class="caption">Listing 5-7: Passing the counts together as a tuple</span>

```console
$ aura run src/main.au
You have read 25.0% of the book.
```

This is better in one way. The function now takes a single argument, so the
two numbers travel together and can't be mixed up with another book's. But
it's worse in another. Inside the function, `progress[1]` and `progress[0]`
say nothing about what they mean. We had to remember that the page count
comes first, and anyone who builds a tuple as `(103, 412)` gets the 400% bug
back. Tuples are fine for a pair that is obvious from context, like a point's
`(x, y)`, or for returning two values from a function. They are a poor home
for data whose parts have different meanings.

## Naming the parts with a class

A class gives each part a name and keeps the parts together:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

def percent_read(book: Book) -> float64:
    return book.pages_read.to_float() / book.pages.to_float() * 100.0

def main():
    dune = Book(title="Dune", pages=412, pages_read=103)
    print(f"You have read {percent_read(dune):.1f}% of {dune.title}.")
```

<span class="caption">Listing 5-8: A `Book` class keeps the counts together and named</span>

```console
$ aura run src/main.au
You have read 25.0% of Dune.
```

Now the function body reads as what it computes, `book.pages_read` over
`book.pages`. The construction call names every field, so the 400% mistake
would mean writing `pages=103, pages_read=412`, which is hard to do by
accident. And because the type has a name, a function that takes a `Book`
can't be handed some other pair of numbers. We also gained a place to put the
title, which the earlier versions had no room for.

`percent_read` takes `book: Book` with no prefix, so it borrows the book with
shared access, which is all it needs. After the call, `main` still owns
`dune` and can read `dune.title` in the same f-string.

## A shelf of books

One book isn't much of a tracker. Let's keep several in a list, add a
function that records some reading, and print a report:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

def percent_read(book: Book) -> float64:
    return book.pages_read.to_float() / book.pages.to_float() * 100.0

def read_pages(book: mut Book, count: int64):
    book.pages_read += count
    if book.pages_read > book.pages:
        book.pages_read = book.pages

def main():
    mut shelf = [
        Book(title="Dune", pages=412, pages_read=103),
        Book(title="Emma", pages=474),
        Book(title="Beloved", pages=324, pages_read=300),
    ]
    read_pages(shelf[1], 50)
    read_pages(shelf[2], 50)
    for book in shelf:
        print(f"{book.title:<10} {percent_read(book):>5.1f}%")
```

<span class="caption">Listing 5-9: Tracking a shelf of books</span>

```console
$ aura run src/main.au
Dune        25.0%
Emma        10.5%
Beloved    100.0%
```

`read_pages` changes the book it's given, so its parameter is `mut Book`.
The call `read_pages(shelf[1], 50)` lends the second element of the list to
the function for the length of the call, and the change lands in the list.
That only works because `shelf` is a `mut` binding. `Emma` used the default
`pages_read` of 0, so after fifty pages it's at 10.5%. `Beloved` would have
gone past its last page, and the clamp in `read_pages` stopped it at 324.

The format specifications line the report up: `:<10` pads the title on the
right to ten characters, and `:>5.1f` right-aligns the percentage in five.

Now let's list the books we've finished. A first attempt collects the titles
into a new list:

```aura fragment
def finished(books: list[Book]) -> list[str]:
    mut titles = list[str]()
    for book in books:
        if book.pages_read == book.pages:
            titles.append(book.title)
    return titles
```

When we add this function to Listing 5-9, after `read_pages`, and print
`finished(shelf)` at the end of `main`, the compiler objects:

```console
$ aura check src/main.au
error[AU3002]: cannot move non-copy field `title` out of borrowed value `book`
 --> src/main.au:18:32
   |
18 |             titles.append(book.title)
   |                                ^
  = related src/main.au:16:9: `book` is borrowed here
  = help: take `book` as `own Book` when the field should be moved, or call `.clone()` on the field to return an independent value
  = fix: replace src/main.au:18:37-18:37 with `.clone()`
```

This is the borrowed-field rule from
[Chapter 4](/book/ch04-02-shared-and-mutable-access) showing up in a real
program. `append` takes ownership of what it stores, and the loop only has
shared access to each `book`, so it can't give the book's title away. The
books still need their titles: the report prints them. So the right fix is
the clone the `fix` line suggests, which puts a new copy of each title in the
result. Here is the whole program with `finished` added:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

def percent_read(book: Book) -> float64:
    return book.pages_read.to_float() / book.pages.to_float() * 100.0

def read_pages(book: mut Book, count: int64):
    book.pages_read += count
    if book.pages_read > book.pages:
        book.pages_read = book.pages

def finished(books: list[Book]) -> list[str]:
    mut titles = list[str]()
    for book in books:
        if book.pages_read == book.pages:
            titles.append(book.title.clone())
    return titles

def main():
    mut shelf = [
        Book(title="Dune", pages=412, pages_read=103),
        Book(title="Emma", pages=474),
        Book(title="Beloved", pages=324, pages_read=300),
    ]
    read_pages(shelf[1], 50)
    read_pages(shelf[2], 50)
    for book in shelf:
        print(f"{book.title:<10} {percent_read(book):>5.1f}%")
    print(f"Finished: {finished(shelf)}")
```

<span class="caption">Listing 5-10: The tracker with a list of finished titles</span>

```console
$ aura run src/main.au
Dune        25.0%
Emma        10.5%
Beloved    100.0%
Finished: [Beloved]
```

Look at the three function signatures together. `percent_read(book: Book)`
reads a book. `read_pages(book: mut Book, count: int64)` changes one.
`finished(books: list[Book])` reads a whole shelf and returns new data. You
can tell what each function does to its arguments without reading a line of
its body, and the compiler holds each one to what it says.

All three functions are about books, though, and nothing in the code ties
them to the `Book` class except their parameter types. In the next section
we'll move them into the class as methods.
