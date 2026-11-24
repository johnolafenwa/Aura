# Methods

A *method* is a function declared inside a class body. It is called on an
instance with a dot, as in `dune.read(50)`, and its first parameter is a
*receiver* that names that instance. You've seen receivers briefly in
[Chapter 4](/book/ch04-02-shared-and-mutable-access). Here we'll
turn the reading tracker's free functions into methods and look at each form
of receiver in turn.

## Defining methods

Here are `percent_read` and `read_pages` from the last section, moved into
`Book`, along with a new `is_finished`:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    def percent_read(self) -> float64:
        return self.pages_read.to_float() / self.pages.to_float() * 100.0

    def is_finished(self) -> bool:
        return self.pages_read == self.pages

    def read(mut self, count: int64):
        self.pages_read += count
        if self.pages_read > self.pages:
            self.pages_read = self.pages

def main():
    mut dune = Book(title="Dune", pages=412)
    dune.read(103)
    print(f"{dune.title}: {dune.percent_read():.1f}%")
    dune.read(1000)
    print(dune.is_finished())
```

<span class="caption">Listing 5-11: `Book` with its first methods</span>

```console
$ aura run src/main.au
Dune: 25.0%
true
```

Each method's body is the free function's body with `book` renamed to
`self`. What changed is the first parameter. `percent_read(book: Book)`
became `percent_read(self)`, and `read_pages(book: mut Book, count: int64)`
became `read(mut self, count: int64)`. The receiver has no type annotation,
because it is always the class the method is declared in.

Calls changed too. `dune.read(103)` passes `dune` as the receiver and `103`
as `count`. As with parameters, the call site doesn't say what kind of
access the method gets. That comes from the receiver in the declaration.

Methods buy us two things over free functions. Everything you can do with a
`Book` now lives in one place, so a reader looking for it knows where to
look. And the names get shorter: `read` is enough, because `dune.read(...)`
already says what is being read.

## The three receivers

The receiver works exactly like the parameter forms from Chapter 4. Bare
`self` is shared access, `mut self` is mutable access, and `own self` takes
ownership.

### `self` reads

`percent_read` and `is_finished` only look at the fields, so they take bare
`self`. The caller keeps its book, and the method can't change it. Try to
write a mutating method with bare `self` and the compiler points at the
first write:

```aura check-fail:AU3003
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    def read(self, count: int64):
        self.pages_read += count
```

```console
$ aura check src/main.au
error[AU3003]: cannot mutate through shared receiver `self`; declare the receiver as `mut self`
 --> src/main.au:7:9
  |
7 |         self.pages_read += count
  |         ^
  = related src/main.au:6:5: shared receiver `self` is declared here
  = help: declare the receiver as `mut self` when the method mutates through `self`
```

Python has no equivalent check. Any method can assign to any attribute of
`self`, so "does this method change the object?" is a question you answer by
reading the body. In Aura it's answered by the signature. That turns out to
matter most when you're reading someone else's class: every method with bare
`self` is safe to call without wondering what it does to your instance.

### `mut self` changes

`read` takes `mut self`, so it can assign to the fields. The instance must be
in a mutable place to call it, which is why `main` declared `mut dune`. As
we saw with `Scoreboard` in Chapter 4, calling a `mut self` method on an
immutable binding fails with "method `read` requires a mutable receiver".

### `own self` consumes

A method that takes `own self` takes the whole instance. After the call, the
caller doesn't have it anymore. That's the right shape for a method that
turns an instance into something else. Here is the simplest example, a
method that gives up the book and keeps only its title:

```aura check-fail:AU3001
class Book:
    title: str
    pages: int64

    def into_title(own self) -> str:
        return self.title

def main():
    dune = Book(title="Dune", pages=412)
    title = dune.into_title()
    print(title)
    print(dune.pages)
```

```console
$ aura check src/main.au
error[AU3001]: use of moved value `dune`
 --> src/main.au:12:11
   |
12 |     print(dune.pages)
   |           ^
  = related src/main.au:10:13: value moved here
  = help: pass shared access when ownership is not needed, or transfer this non-cloneable value only once
```

`into_title` can return `self.title` without cloning it because it owns
`self`: moving a field out of a value you own is allowed. A bare-`self`
method trying the same thing would get the "cannot move non-copy field out
of borrowed value" error from the previous section. The `into_` prefix is a
common convention for consuming methods, so a reader knows to expect the
instance to be gone afterwards.

A more realistic use is building a new value from an old one. When we
finish a book, we might want to turn it into a review that owns the book and
a rating:

*Filename: src/main.au*

```aura
copy class Rating:
    stars: int64

class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    def into_review(own self, stars: int64) -> Review:
        return Review(book=self, rating=Rating(stars=stars))

class Review:
    book: Book
    rating: Rating

impl Display for Book:
    def display(self) -> str:
        return f"{self.title} ({self.pages_read}/{self.pages})"

def main():
    dune = Book(title="Dune", pages=412, pages_read=412)
    print(dune)
    review = dune.into_review(5)
    print(f"{review.book}: {review.rating.stars} stars")
```

<span class="caption">Listing 5-12: A consuming method that moves the book into a review</span>

```console
$ aura run src/main.au
Dune (412/412)
Dune (412/412): 5 stars
```

`into_review` moves `self` into the new `Review`, so after the call the book
lives inside `review`, and `review.book` is how we reach it. There is still
one book, with one owner. (The `impl Display` block controls how a `Book`
prints. We'll come to it at the end of this section.)

### Choosing a receiver

Choose a receiver the same way you choose a parameter's access: the weakest
one that does the job. Start with `self`. Use `mut self` when the method
changes fields. Use `own self` only when the method keeps or dismantles the
instance.

One mistake is worth knowing about, because it looks reasonable to Python
eyes. Writing the receiver with a type annotation, as in `self: Book`, does
not make a receiver:

```aura check-fail:AU3004
class Book:
    title: str
    pages: int64

    def describe(self: Book) -> str:
        return self.title
```

```console
$ aura check src/main.au
error[AU3004]: `self: Type` is not a method receiver; use `self` for shared access, `own self` to consume, or `mut self` to mutate
 --> src/main.au:5:18
  |
5 |     def describe(self: Book) -> str:
  |                  ^
```

## Associated functions

A function in a class body with no receiver at all is an *associated
function*. You call it on the class, not on an instance. Associated
functions are how Aura does what Python does with `@classmethod` and
alternative constructors:

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    def unread(title: own str, pages: int64) -> Book:
        return Book(title=title, pages=pages)

    def started(title: own str, pages: int64, pages_read: int64) -> Book:
        return Book(title=title, pages=pages, pages_read=pages_read)

def main():
    emma = Book.unread("Emma", 474)
    dune = Book.started("Dune", 412, 103)
    print(emma.pages_read)
    print(dune.pages_read)
```

A class can have as many of these as it needs, each with a name that says
what kind of instance it builds. Since an initializer can't fail, an
associated function is also the place for construction that can: one that
returns a `Result` and builds the instance only when its input is valid.
[Chapter 9](/book/ch09-00-error-handling) shows that pattern.

## Properties

Some values are computed from the fields but read like a field. Python marks
those with `@property`, and so does Aura:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    def started(title: own str, pages: int64, pages_read: int64) -> Book:
        return Book(title=title, pages=pages, pages_read=pages_read)

    @property
    def pages_left(self) -> int64:
        return self.pages - self.pages_read

def main():
    dune = Book.started("Dune", 412, 103)
    print(f"{dune.pages_left} pages to go in {dune.title}")
```

<span class="caption">Listing 5-13: A read-only computed property</span>

```console
$ aura run src/main.au
309 pages to go in Dune
```

`dune.pages_left` has no parentheses. Each read calls the getter once and
gives back a fresh value. A property getter must take bare `self` and no
other parameters, since reading a property shouldn't change anything.

So far `pages_left` has only a getter. If we try to set it, the checker tells us where the write would go:

```aura check-fail:AU2045
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    @property
    def pages_left(self) -> int64:
        return self.pages - self.pages_read

def main():
    mut dune = Book(title="Dune", pages=412)
    dune.pages_left = 0
```

```console
$ aura check src/main.au
error[AU2045]: property `pages_left` is read-only and cannot be assigned
 --> src/main.au:12:5
   |
12 |     dune.pages_left = 0
   |     ^
  = help: a property computes a fresh value on each read; bind it to a local to change the copy
```

We can let a reader set how many pages remain. Add this method inside `Book`:

```aura fragment
@pages_left.setter
def pages_left(mut self, value: int64):
    self.pages_read = self.pages - value
```

Now `dune.pages_left = 0` records that we've read all 412 pages. Aura calls the setter on the mutable book. Writing `dune.pages_left += 1` reads the getter once, adds one, and passes the result to the setter. We could validate the requested number in the same method.

## Invariants: what is always true of a book

`pages_left` subtracts `pages_read` from `pages`, and the answer only makes
sense if `pages_read` is somewhere from 0 to `pages`. Every method we've
written assumes that, and none of them says it. Let's put `pages_left`
together with the `read` method from Listing 5-11 and ask `aura verify`, the
command that proves a file's run-time checks can't fail (see
[Contracts and proofs](/book/ch11-04-contracts-and-proofs)), what it makes of
the class:

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0

    @property
    def pages_left(self) -> int64:
        return self.pages - self.pages_read

    def read(mut self, count: int64):
        self.pages_read += count
        if self.pages_read > self.pages:
            self.pages_read = self.pages

def main():
    mut dune = Book(title="Dune", pages=412)
    dune.read(103)
    print(f"{dune.pages_left} pages to go in {dune.title}")
```

```console
$ aura verify src/main.au
error[AU5101]: cannot prove integer result in range
 --> src/main.au:8:21
  |
8 |         return self.pages - self.pages_read
  |                     ^
  = note: in `Book.pages_left` (overflow check)
  = note: counterexample: self.title = "", self.pages = 9223372036854775807, self.pages_read = -1
error[AU5101]: cannot prove integer result in range
 --> src/main.au:11:9
   |
11 |         self.pages_read += count
   |         ^
  = note: in `Book.read` (overflow check)
  = note: counterexample: self.title = "", self.pages = 0, self.pages_read = 9223372036854775807, count = 1
verified src/main.au: 0 of 2 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Each method is checked for every `Book` it could be called on, and the
counterexamples are books that should not exist: one with -1 pages read, and
one with no pages and nine quintillion of them read. The types allow both,
because the fields are plain `int64` values. What rules them out is a fact
about books that we know and haven't written down. A class states such a
fact with an `invariant` line in its body, as we did for the initializer in
[Defining and building classes](/book/ch05-01-defining-classes):

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0
    invariant 0 <= self.pages_read and self.pages_read <= self.pages

    @property
    public def pages_left(self) -> int64:
        ensures 0 <= result and result <= self.pages
        return self.pages - self.pages_read

    public def read(mut self, count: int64):
        requires count >= 0
        self.pages_read += min(count, self.pages - self.pages_read)

def main():
    mut dune = Book(title="Dune", pages=412)
    dune.read(103)
    print(f"{dune.pages_left} pages to go in {dune.title}")
    dune.read(1000)
    print(f"{dune.pages_left} pages to go in {dune.title}")
```

<span class="caption">Listing 5-14: A `Book` that states and keeps its invariant</span>

```console
$ aura run src/main.au
309 pages to go in Dune
0 pages to go in Dune
$ aura verify src/main.au
verified src/main.au: 13 of 13 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Three things changed. The first is the `invariant` line. The second is the
word `public` on both methods. `public` is what lets code in other modules
call a method, which is the subject of
[Chapter 7](/book/ch07-00-modules-and-packages), and it also marks where the
invariant applies. A public method may assume the invariant when it starts,
and a public `mut self` method must leave it true when it returns. A method
without `public` is a helper inside the class. It gets neither the
assumption nor the duty, so it may pass through a state that breaks the rule
as long as the public method that called it puts things right.

The third change is to `read`. With the invariant stated, the verifier
reported that the old body could break it: `read(-1)` on an unread book
walks `pages_read` below zero, and a huge `count` overflows the addition
before the `if` can cap it. So `read` now requires a count that isn't
negative, and adds no more than the pages that are left. The verifier proves
that this body keeps the invariant for every book and every allowed `count`,
and that `pages_left` keeps its own promise, a result from 0 to `pages`.

The invariant guards construction too. A book built as
`Book(title="Torn", pages=90, pages_read=120)` stops the program with
`AU4009` and the text of the invariant, and `aura verify` reports "cannot
prove class invariant holds" on that line without running it.

Two limits are worth knowing now. The verifier assumes the invariant in the
class's own public methods, but not yet in the methods of an `impl` block,
such as the `display` method below, and not for instances it reads out of a
list. Calls made from those places are reported as unproved, and they stay
checked when the program runs.

## Choosing how an instance prints

So far, printing a `Book` has shown the default rendering:
`Book(pages=412, pages_read=0, title=Dune)`. Python lets a class choose its
own text with `__str__`. Aura uses an `impl Display` block, which you saw in
Listing 5-12:

```aura fragment
impl Display for Book:
    def display(self) -> str:
        return f"{self.title} ({self.pages_read}/{self.pages})"
```

The block sits next to the class, not inside it, and defines one method,
`display`, which takes bare `self` and returns a `str`. Once it exists,
`print(book)`, `str(book)`, and `f"{book}"` all use it, and so does printing
a list that contains books. `Display` is a *trait*, a named set of methods a
type can promise to provide. Traits get a chapter of their own,
[Chapter 10](/book/ch10-00-generics-traits-conversions), but this one is
useful enough to reach for now.

## There is no inheritance

If you try to write a subclass the Python way, the parser stops at the
parenthesis:

```aura check-fail:AU1101
class Animal:
    name: str

class Dog(Animal):
    breed: str
```

```console
$ aura check src/main.au
error[AU1101]: expected Colon, found LParen
 --> src/main.au:4:10
  |
4 | class Dog(Animal):
  |          ^
```

Aura classes cannot inherit from other classes. This is a deliberate choice.
Inheritance mixes two things: sharing data (a `Dog` has every field an
`Animal` has) and sharing behavior (a `Dog` can be used where an `Animal` is
expected). In Python both come from one line, along with method resolution
order, `super()` chains, and subclasses that override a method the base class
relied on.

Aura gives you the two pieces separately. To share data, put one class inside
another as a field, as `Review` holds a `Book` in Listing 5-12. To share
behavior, declare a trait and implement it for each type that should have
it, as `Display` does for printing. Composition makes ownership plain: a
`Review` owns its `Book`, and you reach it through `review.book`. Traits let
a function accept any type that provides certain methods without those types
being related.

## Putting it together

Here is the reading tracker rewritten with methods, a factory, a property,
its invariants, and a `Display` implementation that does the report
formatting:

*Filename: src/main.au*

```aura
class Book:
    title: str
    pages: int64
    pages_read: int64 = 0
    invariant self.pages > 0
    invariant 0 <= self.pages_read and self.pages_read <= self.pages

    def unread(title: own str, pages: int64) -> Book:
        requires pages > 0
        return Book(title=title, pages=pages)

    public def percent_read(self) -> float64:
        return self.pages_read.to_float() / self.pages.to_float() * 100.0

    public def is_finished(self) -> bool:
        return self.pages_read == self.pages

    @property
    public def pages_left(self) -> int64:
        ensures 0 <= result and result <= self.pages
        return self.pages - self.pages_read

    public def read(mut self, count: int64):
        requires count >= 0
        self.pages_read += min(count, self.pages - self.pages_read)

impl Display for Book:
    def display(self) -> str:
        return f"{self.title:<10} {self.percent_read():>5.1f}%"

def main():
    mut shelf = [
        Book(title="Dune", pages=412, pages_read=103),
        Book.unread("Emma", 474),
        Book(title="Beloved", pages=324, pages_read=300),
    ]
    shelf[1].read(50)
    shelf[2].read(50)
    mut total_left: int64 = 0
    for book in shelf:
        print(book)
        total_left += book.pages_left
    print(f"{total_left} pages to go")
    for book in shelf:
        if book.is_finished():
            print(f"Finished: {book.title}")
```

<span class="caption">Listing 5-15: The reading tracker built around a `Book` class</span>

```console
$ aura run src/main.au
Dune        25.0%
Emma        10.5%
Beloved    100.0%
733 pages to go
Finished: Beloved
```

`shelf[1].read(50)` calls a `mut self` method on a list element, and the
change lands in the list, as it did when we passed `shelf[1]` to
`read_pages`. The second loop no longer builds a list of finished titles, so
it no longer needs to clone them: it reads each title in place.

The class now carries a second invariant, `self.pages > 0`, because
`percent_read` divides by the page count. `aura verify` proves every check
inside `Book` from the two invariants, the division included, and proves
that `unread` and the two constructions in `main` build books that satisfy
them. It reports the calls in `display` and the calls on `shelf` elements
as unproved, for the two reasons given above, along with the running total,
which has no bound yet. Those checks still run: build a book with no pages,
or call `read` with a negative count, and the program stops with `AU4009`.
The [Manual](/manual/classes) has the full rules for methods, properties, and
initializers, and its [Contracts](/manual/contracts#class-invariants) page
has the rules for invariants.

## Summary

Classes let you give a group of values a name and a fixed shape, and the
compiler checks every construction and field access against it. You can
build instances by field name or position, give fields defaults that are
fresh for every instance, compute fields in `__init__`, and add associated
functions for other ways of building one. Methods put a type's behavior next
to its data, and their receivers say what each one does to the instance:
`self` reads, `mut self` changes, and `own self` consumes. Properties give
computed reads and optional writes. Implementing `Display` controls printing,
and composition and traits take the place of inheritance. An `invariant` line
states what is true of every instance. Aura checks it at construction and
around each public method that changes the instance. Use `aura verify` to prove
that the methods preserve it.

Every field of a class is present at once: a book always has a title, a page
count, and a position. Plenty of data isn't like that. A command is either
"add" or "remove", never both, and a lookup either found something or didn't.
In the next chapter we'll model data like that with enums, and take it apart
with `match`.
