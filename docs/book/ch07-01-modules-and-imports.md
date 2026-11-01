# Modules and imports

Our program for this chapter keeps track of a reading list: which books are
on it, how many pages each has, and how far through the list we are. Let's
start a fresh package for it:

```console
$ aura new shelf
created `shelf`
```

`aura new` gives us the layout you saw in
[Chapter 1](/book/ch01-00-getting-started):

```text
shelf
├── .gitignore
├── Aura.toml
├── src
│   └── main.au
└── tests
    └── smoke.au
```

We'll come back to `Aura.toml` in the [last section of this
chapter](/book/ch07-03-packages-and-workspaces). For now, the only thing that
matters is that `src/` is where our modules live. Replace the contents of
`src/main.au` with Listing 7-1.

*Filename: src/main.au*

```aura
class Book:
    title: str
    author: str
    pages: int64
    read: bool

def pages_left(books: list[Book]) -> int64:
    mut total: int64 = 0
    for book in books:
        if not book.read:
            total += book.pages
    return total

def main() -> int32:
    books = [
        Book(title="Dune", author="Frank Herbert", pages=412, read=true),
        Book(title="Kindred", author="Octavia Butler", pages=264, read=false),
        Book(title="Piranesi", author="Susanna Clarke", pages=272, read=false),
    ]
    print(f"{books.len()} books, {pages_left(books)} pages to go")
    return 0
```

<span class="caption">Listing 7-1: The whole reading list in one file</span>

```console
$ cd shelf
$ aura run src/main.au
3 books, 536 pages to go
```

Nothing here is new. A `Book` class, a function that adds up the pages we
haven't read yet, and a `main` that builds a list and prints a line. But you
can already see where this is heading. The class and its helpers are the part
of the program that knows what a book *is*, and `main` is the part that
decides what to *do* with books today. As the program grows, those two parts
will grow at different speeds, and we'd like them in different files.

## Moving code into a module

Let's move `Book` and `pages_left` into a new file, `src/books.au`, exactly as
they are:

*Filename: src/books.au*

```aura
class Book:
    title: str
    author: str
    pages: int64
    read: bool

def pages_left(books: list[Book]) -> int64:
    mut total: int64 = 0
    for book in books:
        if not book.read:
            total += book.pages
    return total
```

<span class="caption">Listing 7-2: `Book` and `pages_left` in their own file</span>

That file is now a module named `books`. Aura doesn't need any declaration to
make it one: one `.au` file is one module, and its name comes from its path
under `src/`, without the `.au`. So `src/books.au` is the module `books`, and
`src/main.au` is the module `main`.

Code in `main` can't see what's in `books` until it asks for it. We ask with
an `import` statement at the top of the file, and then reach the module's
contents through the module's name, the way you would in Python:

*Filename: src/main.au*

```aura fragment
import books

def main() -> int32:
    books = [
        books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
        books.Book(title="Kindred", author="Octavia Butler", pages=264, read=false),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272, read=false),
    ]
    print(f"{books.len()} books, {books.pages_left(books)} pages to go")
    return 0
```

<span class="caption">Listing 7-3: Importing the `books` module (this won't compile yet)</span>

The caption gives it away. Let's see what the compiler thinks:

```console
$ aura run src/main.au
error[AU2001]: module `books` has no callable member `Book`
 --> src/main.au:5:15
  |
5 |         books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
  |               ^
```

The import itself worked: the compiler found `src/books.au` and knows there is
a module called `books`. What it can't find is anything called `Book` that
`main` is allowed to call, even though the class is right there in the
file.

The reason is that in Aura, everything at the top of a module is private
unless you say otherwise. A private name can be used anywhere inside the file that declares
it, and nowhere else. From `main`'s point of view, a private `Book` and a
`Book` that doesn't exist look the same, which is why the message reads the
way it does. To let other modules use a name, put `public` in front of it.
Let's do that for the class and the function:

*Filename: src/books.au*

```aura fragment
public class Book:
    title: str
    ...

public def pages_left(books: list[Book]) -> int64:
    ...
```

and run again:

```console
$ aura run src/main.au
error[AU2999]: field `title` is private on `Book`
 --> src/main.au:5:20
  |
5 |         books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
  |                    ^
```

Progress. `main` can see the class now, but not its fields. Visibility in
Aura is decided name by name, and a class's fields and methods each have
their own. Marking a class `public` lets other modules name the type; it
doesn't hand them its insides. Because `main` builds books with keyword
arguments, it needs to set every field, so every field has to be public. We'll
question that choice in the [next section](/book/ch07-02-visibility). For now,
mark them all:

*Filename: src/books.au*

```aura
public class Book:
    public title: str
    public author: str
    public pages: int64
    public read: bool

public def pages_left(books: list[Book]) -> int64:
    mut total: int64 = 0
    for book in books:
        if not book.read:
            total += book.pages
    return total
```

<span class="caption">Listing 7-4: Making the class, its fields, and the function public</span>

One more try:

```console
$ aura run src/main.au
error[AU3003]: cannot assign to immutable binding `books`; declare it with `mut` to rebind it
 --> src/main.au:4:5
  |
4 |     books = [
  |     ^
```

This one is a surprise if you're coming from Python, so let's slow down. Look
at what the error points to: line 4, `books = [`. That line was fine in
Listing 7-1. What changed is the `import books` at the top of the file. An
import puts a name into the module's namespace, the same namespace that holds
its functions and classes. Inside `main`, the name `books` already exists, so
`books = [...]` isn't creating a new local binding at all. It's an attempt to
assign to the existing name, and that name is an imported module, which can't
be reassigned. The `mut` hint is the compiler's standard advice for assigning
to an existing binding; it doesn't help here.

Python would let the local list hide the module for the rest of the function,
and the next line, `books.pages_left(books)`, would then fail at run time
because a list has no `pages_left`. Aura won't let one spelling mean two
different things inside a function, so it stops us at the assignment. The fix
is a better name for the list:

*Filename: src/main.au*

```aura fragment
import books

def main() -> int32:
    reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
        books.Book(title="Kindred", author="Octavia Butler", pages=264, read=false),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272, read=false),
    ]
    left = books.pages_left(reading_list)
    print(f"{reading_list.len()} books, {left} pages to go")
    return 0
```

<span class="caption">Listing 7-5: `main` using the `books` module</span>

```console
$ aura run src/main.au
3 books, 536 pages to go
```

The same output as before, from two files. Notice that we still pass only
`src/main.au` to `aura run`. The compiler starts from the file you name, finds
the `Aura.toml` above it, treats the `src/` directory beside that manifest as
the *source root*, and loads whatever the imports ask for. You never list the
other files anywhere.

## How paths map to modules

Let's add a second module, one that draws a text progress bar. We'll put it
one level down, in a `report` directory, because we expect more reporting
code to join it later:

*Filename: src/report/progress.au*

```aura
public def progress_bar(done: int64, total: int64) -> str:
    width: int64 = 20
    filled = done * width // total
    return "[" + "#".repeat(filled) + "-".repeat(width - filled) + "]"
```

<span class="caption">Listing 7-6: A progress bar module in a subdirectory</span>

We'll also add a `total_pages` function to `src/books.au`, so `main` can work
out how much we've read:

*Filename: src/books.au*

```aura fragment
public def total_pages(books: list[Book]) -> int64:
    mut total: int64 = 0
    for book in books:
        total += book.pages
    return total
```

The project now looks like this:

```text
shelf
├── Aura.lock
├── Aura.toml
├── src
│   ├── books.au
│   ├── main.au
│   └── report
│       └── progress.au
└── tests
    └── smoke.au
```

(The `Aura.lock` file appeared the first time we ran the program. It records
how the compiler resolved our package, and we'll read it in [section
7.3](/book/ch07-03-packages-and-workspaces).)

The module name for `src/report/progress.au` is its path under `src/` with
dots in place of slashes: `report.progress`. You might try importing the
directory, the way a Python package with an `__init__.py` lets you:

```aura fragment
import books
import report
```

```console
$ aura run src/main.au
error[AU2999]: cannot resolve module `report` at `src/report.au`
 --> src/main.au:2:1
  |
2 | import report
  | ^
```

The message tells us exactly where the compiler looked: for `import report`
it wanted a file called `src/report.au`. A directory on its own is not a
module in Aura, and there is no `__init__` file. Every import names a file:

```aura fragment
import books
import report.progress

def main() -> int32:
    reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
        books.Book(title="Kindred", author="Octavia Butler", pages=264, read=false),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272, read=false),
    ]
    total = books.total_pages(reading_list)
    done = total - books.pages_left(reading_list)
    print(report.progress.progress_bar(done, total))
    return 0
```

```console
$ aura run src/main.au
[########------------]
```

That works, but `report.progress.progress_bar(done, total)` is a mouthful.
An import of a dotted path binds its *first* component, `report`, and you
spell out the rest every time you use something from it. That's useful when
the full path helps the reader, and tedious when it doesn't.

## Importing names directly

When one or two names from a module carry most of the weight in a file,
import them directly with `from ... import ...`:

*Filename: src/main.au*

```aura fragment
import books
from report.progress import progress_bar

def main() -> int32:
    reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
        books.Book(title="Kindred", author="Octavia Butler", pages=264, read=false),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272, read=false),
    ]
    total = books.total_pages(reading_list)
    done = total - books.pages_left(reading_list)
    print(f"{progress_bar(done, total)} {done}/{total} pages")
    return 0
```

<span class="caption">Listing 7-7: Mixing a module import and a direct import</span>

```console
$ aura run src/main.au
[########------------] 412/948 pages
```

A from-import binds only the names you list. After this line `progress_bar`
is a name in `main`, but `report` is not, so `report.progress.progress_bar`
would no longer resolve. You can import any public function, class, enum,
trait, or module constant this way.

Listing 7-7 uses both styles on purpose. `books.Book`
and `books.total_pages` read well with their prefix: it tells the reader
which part of the program a name belongs to, and keeps `Book` from looking
like something `main` defined. `progress_bar` is a single, distinctive name
that `main` uses once, and the prefix would add nothing. A good default is to
keep the module prefix when you use several things from a module and to
import directly when one name is the point.

Each entry of a from-import can take its own spelling with `as`, and a long
list can go in parentheses across several lines, with a trailing comma:

```aura fragment
from books import (
    Book,
    pages_left,
    total_pages as count_pages,
)
```

`as` works on a whole module too. This is handy for long paths:

```aura fragment
import books
import report.progress as progress

def main() -> int32:
    reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412, read=true),
        books.Book(title="Kindred", author="Octavia Butler", pages=264, read=false),
    ]
    total = books.total_pages(reading_list)
    print(progress.progress_bar(412, total))
    print(report.progress.progress_bar(412, total))
    return 0
```

```console
$ aura run src/main.au
error[AU2001]: unknown name `report`
 --> src/main.au:11:11
   |
11 |     print(report.progress.progress_bar(412, total))
   |           ^
```

An aliased module import binds only the alias. `progress.progress_bar` works,
and `report` is unknown, because nothing put it in scope. An alias changes
only how *this* file spells the name. The function is still
`report.progress.progress_bar` everywhere else, with the same type, the same
visibility, and the same documentation.

## One name, one meaning

Imports share a namespace with everything else at the top of a module. That
rule caught us with the `books` list earlier, and it applies to declarations
too. Suppose `main` imports `progress_bar` and then defines its own:

```aura fragment
from report.progress import progress_bar

def progress_bar(done: int64, total: int64) -> str:
    return f"{done} of {total}"
```

```console
$ aura run src/main.au
error[AU2999]: duplicate item `progress_bar` (previously declared as function at 1:8)
 --> src/main.au:3:1
  |
3 | def progress_bar(done: int64, total: int64) -> str:
  | ^
```

The "1:8" in the message is the position of the original declaration inside
`src/report/progress.au`, line 1, column 8, right after `public`; the message
doesn't name that file, so it's worth knowing where to look. Python would
let the second definition replace the imported one without a word. Aura
treats it as a mistake, because a reader who sees `progress_bar` in this file
should be able to tell which function it is. If you really want both, give
one of them another name with `as`.

## An import is not a copy

It's tempting to think of `import` as pasting another file's text into this
one. It isn't, and the difference shows up as soon as two modules define
things with the same name. Let's try defining a second `Book` class in `main`
and handing a list of them to `books.pages_left`:

*Filename: src/main.au*

```aura fragment
import books

class Book:
    title: str
    author: str
    pages: int64
    read: bool

def main() -> int32:
    mine = [Book(title="Dune", author="Frank Herbert", pages=412, read=false)]
    print(books.pages_left(mine))
    return 0
```

```console
$ aura run src/main.au
error[AU2002]: argument type mismatch for function `pages_left`: expected `books.Book`, found `Book`
 --> src/main.au:11:28
   |
11 |     print(books.pages_left(mine))
   |                            ^
```

The two classes have the same name and the same fields, but they are
different types. Every declaration keeps the identity of the module that
defines it, so the class in `books` is `books.Book` wherever you use it, and
the compiler says so in the message. This is what makes private names
possible at all: the compiler always knows which module a declaration belongs
to, and so it can tell whether the code reaching for it is inside that module
or outside.

## What happens when a module is imported

In Python, importing a module runs it. Every top-level statement executes the
first time something imports the file, which is how modules set up global
state, and also how an innocent `import` can open a network connection or
print to the terminal. Aura doesn't do that. Let's add a stray `print` to the
bottom of `src/report/progress.au`:

*Filename: src/report/progress.au*

```aura
public def progress_bar(done: int64, total: int64) -> str:
    width: int64 = 20
    filled = done * width // total
    return "[" + "#".repeat(filled) + "-".repeat(width - filled) + "]"

print("loading report.progress")
```

```console
$ aura run src/main.au
[########------------] 412/948 pages
$ aura run src/report/progress.au
loading report.progress
```

Importing the module gave `main` its declarations and nothing else. The
`print` only runs when `progress.au` is itself the program we asked to run.
So you can read an Aura file's import list and know that none of those
imports will do anything at run time. The one thing that does happen before
`main` starts is that module *constants* get their values, which we'll meet
in the next section.

Aura also needs the imports between modules to form a tree with no loops,
so every module can be checked after the modules it depends on. If `books.au` imports
`report.progress` and `report/progress.au` imports `books`, the compiler
refuses the whole program:

```console
$ aura run src/main.au
error[AU2999]: cyclic import involving `src/books.au`
 --> src/main.au
```

When you hit this, the usual fix is to move whatever both modules need into
a third module that both of them import.

We now have three modules that can see each other's public names. So far we
made things public whenever the compiler asked, which got the program
running but gave `main` the run of `Book`'s insides. In the next section
we'll go back and decide what each module should really expose.
