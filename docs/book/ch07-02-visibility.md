# Visibility and private names

At the end of the last section, every field of `Book` was public because the
compiler asked us to make it so. That got the program running, but it also
means any module that imports `books` can build a `Book` with any values it
likes and change those values whenever it wants. For a class with four plain
fields that might seem harmless. Let's give `Book` a field where it isn't.

So far a book is either read or not. A reading tracker ought to know how far
into a book we are, so let's replace `read: bool` with a bookmark: the number
of pages we've read. A bookmark only makes sense between zero and the book's
page count. If a module can write `bookmark=9000` into a 412-page book, then
`pages_left` will happily return a negative number, and the bug will show up
far away from the line that caused it. We want `books` to be the only module
that can move a bookmark, so it can keep that promise.

In Python, you'd signal this by naming the field `_bookmark` and hoping
everyone respects the underscore. Aura makes it a rule the compiler checks.

## Private fields

A field without `public` is private. Let's write the new `Book` that way, and
give it a method that moves the bookmark safely:

*Filename: src/books.au*

```aura
public class Book:
    public title: str
    public author: str
    public pages: int64
    bookmark: int64

    public def read_to(mut self, page: int64):
        self.bookmark = max(0, min(page, self.pages))

    public def is_finished(self) -> bool:
        return self.bookmark == self.pages

public def pages_left(books: list[Book]) -> int64:
    mut total: int64 = 0
    for book in books:
        total += book.pages - book.bookmark
    return total

public def total_pages(books: list[Book]) -> int64:
    mut total: int64 = 0
    for book in books:
        total += book.pages
    return total
```

<span class="caption">Listing 7-8: `Book` with a private bookmark (this won't work from `main` yet)</span>

`read_to` clamps whatever page it's given into the range `0` to `pages`, using
the two-argument forms of `min` and `max`. `pages_left` reads
`book.bookmark` directly, and that's fine: it lives in the same module as the
class, and inside its own module a private name is as usable as a public one.
Visibility is about module boundaries, not about classes. There's no
equivalent of a "private to this class" field that a free function in the
same file can't touch.

Now `main` stops passing `read=...` and records progress with `read_to`:

*Filename: src/main.au*

```aura fragment
import books
from report.progress import progress_bar

def main() -> int32:
    mut reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412),
        books.Book(title="Kindred", author="Octavia Butler", pages=264),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272),
    ]
    reading_list[0].read_to(412)
    reading_list[1].read_to(100)
    total = books.total_pages(reading_list)
    done = total - books.pages_left(reading_list)
    print(f"{progress_bar(done, total)} {done}/{total} pages")
    return 0
```

<span class="caption">Listing 7-9: `main` moving bookmarks through a method</span>

```console
$ aura run src/main.au
error[AU2999]: class constructor `Book` cannot initialize private field `bookmark` from another module
 --> src/main.au:6:15
  |
6 |         books.Book(title="Dune", author="Frank Herbert", pages=412),
  |               ^
```

This is the other side of making a field private. `main` isn't allowed to set
`bookmark`, but the constructor has to give it *some* value, and `main` is the
one calling the constructor. With no default and no permission, there's no
way for this call to produce a complete `Book`.

There are two ways out. One is a public method on `Book` that builds the
object from inside the module, where private fields are reachable; you'll see
that pattern in many libraries. The other, which fits here, is a default
value. A new book starts with the bookmark at page zero, so let's say so:

```aura fragment
    bookmark: int64 = 0
```

```console
$ aura run src/main.au
[##########----------] 512/948 pages
```

We've read all 412 pages of *Dune* and 100 of *Kindred*, which is 512 of 948.
Let's check that the compiler really keeps `main` away from the bookmark.
First, passing it to the constructor:

```aura fragment
        books.Book(title="Dune", author="Frank Herbert", pages=412, bookmark=412),
```

```console
$ aura run src/main.au
error[AU2999]: field `bookmark` is private on `Book`
 --> src/main.au:6:69
  |
6 |         books.Book(title="Dune", author="Frank Herbert", pages=412, bookmark=412),
  |                                                                     ^
```

and then assigning it directly:

```aura fragment
    reading_list[1].bookmark = 100
```

```console
$ aura run src/main.au
error[AU2999]: field `bookmark` is private on `Book`
 --> src/main.au:11:5
   |
11 |     reading_list[1].bookmark = 100
   |     ^
```

Both are refused with the same message, which names the field and the class.
The only way for `main` to move a bookmark now is `read_to`, so we can try the
case that worried us:

```aura fragment
    reading_list[1].read_to(9000)
```

```console
$ aura run src/main.au
[##############------] 676/948 pages
```

*Kindred* counts as finished at 264 pages, not 9000, and `pages_left` can't
go negative. The promise "a bookmark stays inside its book" is now kept by
the code in `books.au`, and no amount of code elsewhere can break it.
If we later decide to store the bookmark differently, say as a percentage,
we can change `books.au` alone, because nothing outside it ever saw the field.

Keep in mind that a `public` field is public for writing as well as reading.
`main` could still write `reading_list[1].pages = 50` and shrink a book. If
that ever matters, the same treatment applies: make the field private and
expose a method that reads it.

## Private methods and functions

Methods follow the same rule. Suppose `read_to` grows complicated enough that
we pull the clamping out into its own method:

```aura fragment
    public def read_to(mut self, page: int64):
        self.bookmark = self.clamp(page)

    def clamp(self, page: int64) -> int64:
        return max(0, min(page, self.pages))
```

`clamp` has no `public`, so it's a detail of the `books` module. If `main`
tries to call it, say with `print(reading_list[1].clamp(9000))`, the compiler
refuses:

```console
$ aura run src/main.au
error[AU2999]: method `clamp` is private on `Book`
 --> src/main.au:11:27
   |
11 |     print(reading_list[1].clamp(9000))
   |                           ^
```

Top-level functions work the same way. Here is `report.progress` with a
private helper:

*Filename: src/report/progress.au*

```aura
public def progress_bar(done: int64, total: int64) -> str:
    width: int64 = 20
    filled = done * width // total
    return "[" + segment("#", filled) + segment("-", width - filled) + "]"

def segment(mark: str, count: int64) -> str:
    return mark.repeat(count)
```

<span class="caption">Listing 7-10: A private helper beside a public function</span>

If `main` tries to import the helper by name, the compiler tells us exactly
what's wrong:

```aura fragment
from report.progress import progress_bar, segment
```

```console
$ aura run src/main.au
error[AU2999]: item `segment` is private in module `report.progress`
 --> src/main.au:2:43
  |
2 | from report.progress import progress_bar, segment
  |                                           ^
```

Reaching for a private item through the module prefix, as in
`report.progress.segment(...)`, gives the "has no callable member" message we
met in the last section. Both mean the same thing: the name exists, but not
for you.

An alias doesn't get around any of this. `from report.progress import segment
as seg` fails the same way, because an alias renames a name you're already
allowed to use; it doesn't change who is allowed.

## Module constants

The progress bar's width is buried inside the function. Let's give it, and
the heading we print above the bar, a home of their own. A binding at the top
level of a module is a *module constant*:

*Filename: src/settings.au*

```aura
public shelf_name = "Reading list"
public bar_width: int64 = 20
```

<span class="caption">Listing 7-11: Constants in their own module</span>

Constants follow the same visibility rule as everything else, so we mark both
`public`. `report.progress` can now use the shared width:

*Filename: src/report/progress.au*

```aura fragment
import settings

public def progress_bar(done: int64, total: int64) -> str:
    filled = done * settings.bar_width // total
    empty = settings.bar_width - filled
    return "[" + segment("#", filled) + segment("-", empty) + "]"

def segment(mark: str, count: int64) -> str:
    return mark.repeat(count)
```

and `main` imports `settings` too, to print the heading:

*Filename: src/main.au*

```aura fragment
import books
import settings
from report.progress import progress_bar

def main() -> int32:
    mut reading_list = [
        books.Book(title="Dune", author="Frank Herbert", pages=412),
        books.Book(title="Kindred", author="Octavia Butler", pages=264),
        books.Book(title="Piranesi", author="Susanna Clarke", pages=272),
    ]
    reading_list[0].read_to(412)
    reading_list[1].read_to(100)
    total = books.total_pages(reading_list)
    done = total - books.pages_left(reading_list)
    print(settings.shelf_name)
    print(f"{progress_bar(done, total)} {done}/{total} pages")
    return 0
```

<span class="caption">Listing 7-12: Two modules sharing the `settings` constants</span>

```console
$ aura run src/main.au
Reading list
[##########----------] 512/948 pages
```

Two modules import `settings`, and that's fine. Each module is loaded and
initialized once, however many files import it. The compiler gives every
reachable module's constants their values before `main` starts, dependencies
first, so by the time `progress_bar` runs, `settings.bar_width` is already 20.
Within one module, constants are set in the order they're written, and one
constant can use an earlier one but not a later one:

```aura fragment
public shelf_name = "Reading list"
public bar_width: int64 = full_width - 4
full_width: int64 = 24
```

```console
$ aura run src/main.au
error[AU2001]: module constant `full_width` is used before initialization
 --> src/settings.au:2:27
  |
2 | public bar_width: int64 = full_width - 4
  |                           ^
  = related src/settings.au:3:1: `full_width` is declared here
```

Swap the two lines and it compiles, with `full_width` usable inside
`settings.au` but invisible outside it.

### Constants really are constant

A module constant can't change. That's true from inside its module and from
outside it. Here's `main` trying to widen the bar at run time:

```aura fragment
    settings.bar_width = 30
```

```console
$ aura run src/main.au
error[AU3003]: cannot assign through immutable place `settings.bar_width`
 --> src/main.au:15:5
   |
15 |     settings.bar_width = 30
   |     ^
```

and a module can't declare a mutable top-level binding in the first place:

```aura check-fail:AU3003
mut pages_read: int64 = 0

def main():
    print(pages_read)
```

```console
$ aura check src/main.au
error[AU3003]: module bindings are immutable; `mut` module state is not supported
 --> src/main.au:1:5
  |
1 | mut pages_read: int64 = 0
  |     ^
  = help: put mutable state in a local value owned by `main` or another explicit owner
```

The help line is the whole design in one sentence. Python modules are a
common place to keep global state: a cache, a counter, a configuration
object that gets patched in tests. Every function that touches that state is
quietly coupled to every other one, and once tasks run in parallel (Chapter
15), shared mutable globals become data races. Aura removes the option.
Mutable state lives in a value with an owner, usually created in `main`, and
it's passed to the functions that need it, where their signatures say whether
they read it or change it.

One more consequence follows from constants belonging to their module. A
constant like `shelf_name` is an owned `str` stored in `settings`, and other
modules get shared access to it, never ownership:

```aura fragment
    title = settings.shelf_name
```

```console
$ aura run src/main.au
error[AU3001]: cannot move `settings.shelf_name` out of immutable module storage
 --> src/main.au:15:22
   |
15 |     title = settings.shelf_name
   |                      ^
  = help: keep shared access or construct an independent owned value
```

`title = ...` would make `main` the new owner of the string, which would leave
`settings` without its constant. You can use the constant directly, as
`print(settings.shelf_name)` does, or ask for your own copy with
`settings.shelf_name.clone()`. Copy values such as `bar_width` have no such
issue; reading one gives you a copy.

## Documenting the public surface

Once a module has a deliberate public surface, it's worth saying what that
surface is for. A docstring at the top of the file describes the module, and
one as the first line of each public declaration describes that declaration:

*Filename: src/books.au*

```aura
"""Books on the reading list, and the page counts we track for them."""

public class Book:
    """One book, and how far into it we are."""

    public title: str
    public author: str
    public pages: int64
    bookmark: int64 = 0

    public def read_to(mut self, page: int64):
        """Move the bookmark to `page`, keeping it inside the book."""
        self.bookmark = max(0, min(page, self.pages))

    public def is_finished(self) -> bool:
        """Report whether the bookmark has reached the last page."""
        return self.bookmark == self.pages

public def pages_left(books: list[Book]) -> int64:
    """Count the pages we have not read yet, across every book."""
    mut total: int64 = 0
    for book in books:
        total += book.pages - book.bookmark
    return total

public def total_pages(books: list[Book]) -> int64:
    """Count every page on the list."""
    mut total: int64 = 0
    for book in books:
        total += book.pages
    return total
```

<span class="caption">Listing 7-13: The `books` module with docstrings</span>

Your editor shows these when you hover over `books.Book` in `main`, and
`aura doc` turns them into a reference page:

```console
$ aura doc src/books.au --out api
wrote api/books.md
```

The page lists `Book` with its `title`, `author`, and `pages` fields, its two
public methods, and the two public functions. `bookmark` isn't there. The
documentation shows exactly what other modules can use, which is also exactly
what you're promising to keep working when you change the module.

That last point is the best guide to choosing what to make public. Every
public name is something other code may come to depend on, and changing it
means changing them. A private name can be renamed, rewritten, or deleted
with only one file to check. So start private, and make a name public when
another module has a real reason to use it. For classes, that usually means
public methods that describe behavior, like `read_to` and `is_finished`, and
private fields that hold the representation.

Our modules now have clean boundaries, but they all live inside one package.
The progress bar has nothing to do with books, and it would be useful in
other programs. In the next section we'll move it into a package of its own
and make `shelf` depend on it.
