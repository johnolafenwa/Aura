# Traits

A trait names a set of methods that a type can promise to provide. Code that
works with "any type that has these methods" asks for the trait, and the
compiler checks the promise on both sides: that each type claiming the trait
really has the methods, and that generic code only calls methods the trait
provides. If you've used Python's abstract base classes, Go's interfaces, or
Rust's traits, the idea will be familiar. The Aura version is closest to
Rust's.

We'll leave `largest` broken for a moment and build up traits with a different
example: a small shop that sells books and coffee. Different items are priced
in different ways, but the till only needs to know a name and a price.

## Declaring and implementing a trait

Here's a trait for things with a price, and two classes that implement it:

*Filename: src/main.au*

```aura
trait Priced:
    def name(self) -> str
    def price_cents(self) -> int64

    def label(self) -> str:
        return f"{self.name()}: {self.price_cents()} cents"

class Book:
    title: str
    pages: int64

class Coffee:
    size: str
    shots: int64

impl Priced for Book:
    def name(self) -> str:
        return self.title.clone()

    def price_cents(self) -> int64:
        return 500 + self.pages * 2

impl Priced for Coffee:
    def name(self) -> str:
        return f"{self.size} coffee"

    def price_cents(self) -> int64:
        return 300 + self.shots * 75

    def label(self) -> str:
        return f"{self.name()} ({self.shots} shots): {self.price_cents()} cents"

def main():
    novel = Book(title="Emma", pages=400)
    latte = Coffee(size="large", shots=2)
    print(novel.price_cents())
    print(novel.label())
    print(latte.label())
```

<span class="caption">Listing 10-5: A `Priced` trait with two required methods and one default method</span>

```console
$ aura run src/main.au
1300
Emma: 1300 cents
large coffee (2 shots): 450 cents
```

The `trait` block lists method signatures. `name` and `price_cents` end at the
signature, with no body: they're *required*, and every implementation must
supply them. `label` has a body, which makes it a *default* method. An
implementation gets it for free and may replace it. A default method can call
the trait's other methods, as `label` does, because every type that implements
`Priced` is guaranteed to have them.

An `impl Priced for Book:` block is where `Book` keeps its promise. `Book` uses
the default `label`. `Coffee` overrides it to show the number of shots. Once
the implementation exists, the trait's methods are called like any other
method: `novel.label()`.

`Book.name` returns `self.title.clone()`. The method has shared access to
`self`, so it can't hand out the title it doesn't own; `clone` makes an
independent owned `str` to return.

The compiler holds implementations to the trait's contract. Leave out a
required method and the `impl` block is rejected where it starts:

```aura check-fail:AU2999
trait Priced:
    def price_cents(self) -> int64
    def name(self) -> str

class Sticker:
    design: str

impl Priced for Sticker:
    def price_cents(self) -> int64:
        return 99
```

```console
$ aura check src/main.au
error[AU2999]: impl of `Priced` for `Sticker` is missing method `name`
 --> src/main.au:8:1
  |
8 | impl Priced for Sticker:
  | ^
```

Changing a signature is rejected too, even a change that looks harmless, like
returning `int32` where the trait says `int64`:

```aura check-fail:AU2999
trait Priced:
    def price_cents(self) -> int64

class Sticker:
    design: str

impl Priced for Sticker:
    def price_cents(self) -> int32:
        return 99
```

```console
$ aura check src/main.au
error[AU2999]: method `price_cents` in impl of `Priced` does not match the trait signature
 --> src/main.au:8:5
  |
8 |     def price_cents(self) -> int32:
  |     ^
```

The receiver, the parameter types and their ownership modes, and the return
type all have to match. That strictness is what lets generic code rely on a
trait without seeing the implementations.

## Trait bounds

Now for the payoff. A generic function can require that its type parameter
implements a trait by writing a *bound* after the parameter: `T: Priced`.
Inside the function, any method of `Priced` can be called on a `T`:

```aura
trait Priced:
    def name(self) -> str
    def price_cents(self) -> int64

    def label(self) -> str:
        return f"{self.name()}: {self.price_cents()} cents"

class Book:
    title: str
    pages: int64

class Coffee:
    size: str
    shots: int64

impl Priced for Book:
    def name(self) -> str:
        return self.title.clone()

    def price_cents(self) -> int64:
        return 500 + self.pages * 2

impl Priced for Coffee:
    def name(self) -> str:
        return f"{self.size} coffee"

    def price_cents(self) -> int64:
        return 300 + self.shots * 75

    def label(self) -> str:
        return f"{self.name()} ({self.shots} shots): {self.price_cents()} cents"

def total_cents[T: Priced](items: list[T]) -> int64:
    mut total = 0
    for item in items:
        total += item.price_cents()
    return total

def print_receipt[T: Priced](items: list[T]):
    for item in items:
        print(item.label())
    print(f"total: {total_cents(items)} cents")

def main():
    books = [Book(title="Dune", pages=600), Book(title="Emma", pages=400)]
    print_receipt(books)
    order: list[Book | Coffee] = [Book(title="Dune", pages=600), Coffee(size="large", shots=2)]
    print_receipt(order)
```

<span class="caption">Listing 10-6: Generic functions bounded by `Priced`</span>

```console
$ aura run src/main.au
Dune: 1700 cents
Emma: 1300 cents
total: 3000 cents
Dune: 1700 cents
large coffee (2 shots): 450 cents
total: 2150 cents
```

The bound works in both directions. Inside `total_cents`, it's what allows
`item.price_cents()`. Without it, the compiler has no reason to think a `T` has
that method:

```aura check-fail:AU2999
def total_cents[T](items: list[T]) -> int64:
    mut total = 0
    for item in items:
        total += item.price_cents()
    return total
```

```console
$ aura check src/main.au
error[AU2999]: unsupported method call `price_cents` on `T`
 --> src/main.au:4:23
  |
4 |         total += item.price_cents()
  |                       ^
```

At the call site, the bound is a requirement the argument must meet.
`print_receipt(books)` is fine because `Book` implements `Priced`.

The second call is more interesting. `order` is a list of `Book | Coffee`, a
union of two types. A union satisfies a bound when every member implements the
trait with the same method signatures, so `print_receipt` accepts it, and each
`item.label()` call runs the implementation for whichever type that element
actually is. That's how you put different kinds of items in one list. Aura has
no "trait object" type that means "any `Priced` thing"; the union names the
exact set of types, and the compiler checks every one of them.

### Traits are nominal

Here's a mistake that comes naturally to a Python programmer. A `Sticker` class
has a `price_cents` method with exactly the right signature, but nobody wrote
`impl Priced for Sticker`:

```aura check-fail:AU2002
trait Priced:
    def price_cents(self) -> int64

class Sticker:
    design: str

    def price_cents(self) -> int64:
        return 99

def total_cents[T: Priced](items: list[T]) -> int64:
    mut total = 0
    for item in items:
        total += item.price_cents()
    return total

def main():
    print(total_cents([Sticker(design="cat")]))
```

```console
$ aura check src/main.au
error[AU2002]: type `Sticker` does not implement trait `Priced`
 --> src/main.au:17:11
   |
17 |     print(total_cents([Sticker(design="cat")]))
   |           ^
```

In Python's duck typing, having the method would be enough. In Aura a type
implements a trait only through an `impl` block, never because its method names
happen to line up. A method called `price_cents` on a sticker might mean
something unrelated to the till's idea of a price, and `impl Priced for
Sticker` is where someone states that it means the same thing. It's one line of
ceremony in exchange for never being surprised by an accidental match.

### Several bounds, and traits that build on traits

A type parameter can have more than one bound, joined with `+`, as in `T:
Priced + Display`. The function can then call the methods of every trait in the
list, and the argument must implement all of them. We'll use that form shortly.

A trait can also require another trait. If every priced thing needs a name, we
can split `name` into its own trait and make `Priced` depend on it:

```aura
trait Named:
    def name(self) -> str

trait Priced: Named:
    def price_cents(self) -> int64

    def label(self) -> str:
        return f"{self.name()}: {self.price_cents()} cents"

class Book:
    title: str
    pages: int64

impl Priced for Book:
    def price_cents(self) -> int64:
        return 500 + self.pages * 2

impl Named for Book:
    def name(self) -> str:
        return self.title.clone()

def show[T: Priced](item: T):
    print(item.name())
    print(item.label())

def main():
    show(Book(title="Emma", pages=400))
```

```console
$ aura run src/main.au
Emma
Emma: 1300 cents
```

`trait Priced: Named:` reads "`Priced` requires `Named`". The second colon ends
the header. `Named` is a *supertrait* of `Priced`, so the default `label` can
call `self.name()`, and a bound of `T: Priced` gives `show` access to `name` as
well. Implementing `Priced` doesn't implement `Named` for you, though. If we
delete the `impl Named for Book` block, the compiler reports `impl of `Priced`
for `Book` requires supertrait `Named``.

## Traits the language knows about

A few traits have names the compiler recognizes, and implementing them changes
how your type behaves with the language's own syntax.

### `Display`: how a value prints

Prices in cents are awkward to read. Let's give them a type of their own:

```aura
copy class Money:
    cents: int64

impl Display for Money:
    def display(self) -> str:
        return f"${self.cents // 100}.{self.cents % 100:02}"

def main():
    price = Money(cents=1250)
    print(price)
    print(f"due today: {price}")
    print([price, Money(cents=5)])
    print(str(price).len())
```

```console
$ aura run src/main.au
$12.50
due today: $12.50
[$12.50, $0.05]
6
```

`Display` is part of every module's prelude, so we can implement it without
declaring it. Its one method, `display`, returns the text for the value.
`print`, `str()`, and f-strings all use it, and so does printing a `Money`
inside a list, which is why the third line shows `$12.50` instead of
`Money(cents=1250)`. A type without a `Display` implementation keeps the
structural rendering you've seen all along.

`Money` is a `copy class`, which makes its values copy values like integers. It
holds a single integer, so copying it is as cheap as copying the integer, and
we're about to pass prices around freely.

You can implement `Display` only for a class or enum declared in the same
module, and only once per type. One type gets one rendering, decided by the
module that owns it.

### Operators

Adding two `Money` values seems natural, but the compiler doesn't know what `+` means for them:

```aura check-fail:AU2999
copy class Money:
    cents: int64

def main():
    print(Money(cents=1300) + Money(cents=450))
```

```console
$ aura check src/main.au
error[AU2999]: unsupported operands for binary expression: `Money`
 --> src/main.au:5:11
  |
5 |     print(Money(cents=1300) + Money(cents=450))
  |           ^
```

When no builtin rule applies, Aura turns `a + b` into a call to the `add`
method of a trait named `Add`. Unlike `Display`, the operator traits aren't
predeclared, so a module that wants one declares it with the shape the compiler
expects:

```aura
trait Add[Rhs, Out]:
    def add(self, rhs: Rhs) -> Out

copy class Money:
    cents: int64

impl Display for Money:
    def display(self) -> str:
        return f"${self.cents // 100}.{self.cents % 100:02}"

impl Add[Money, Money] for Money:
    def add(self, rhs: Money) -> Money:
        return Money(cents=self.cents + rhs.cents)

def main():
    book = Money(cents=1300)
    coffee = Money(cents=450)
    print(book + coffee)
    mut total = Money(cents=0)
    for price in [book, coffee, coffee]:
        total += price
    print(total)
```

<span class="caption">Listing 10-7: Implementing `Add` gives `Money` a `+` operator</span>

```console
$ aura run src/main.au
$17.50
$22.00
```

`Add` is a generic trait with two parameters: the type on the right of the `+`
and the type of the result. `impl Add[Money, Money] for Money` says "a `Money`
plus a `Money` is a `Money`". Once that exists, `book + coffee` calls our
`add`, and `total += price` works too.

The other operators follow the same pattern: `Sub`, `Mul`, `Div`, `FloorDiv`,
and `Mod` for `-`, `*`, `/`, `//`, and `%`, `Neg` and `Not` for unary `-` and
`not`, and `Ord` for the comparisons `<`, `<=`, `>`, and `>=`. The
[Manual](/manual/generics-and-traits) lists each trait's exact shape. Two
things stay fixed: `==` and `!=` always use Aura's builtin structural equality,
and `and` and `or` can't be overloaded.

## Fixing `largest`

We now have everything `largest` needs. Its body uses `>`, and `>` on a type
parameter is a call to `Ord.gt`. So the bound we want is `T: Ord[T]`: "a `T`
can be compared with another `T`". Let's write it, and call it with integers
first:

```aura check-fail:AU2002
trait Ord[Rhs]:
    def lt(self, rhs: Rhs) -> bool
    def le(self, rhs: Rhs) -> bool
    def gt(self, rhs: Rhs) -> bool
    def ge(self, rhs: Rhs) -> bool

def largest[T: Ord[T]](values: own list[T]) -> T | None:
    mut rest = values
    if rest.len() == 0:
        return None
    mut best = rest.pop()
    while rest.len() > 0:
        candidate = rest.pop()
        if candidate > best:
            best = candidate
    return best

def main():
    print(largest([3, 9, 4]))
```

```console
$ aura check src/main.au
error[AU2002]: type `int64` does not implement trait `Ord[int64]`
 --> src/main.au:19:11
   |
19 |     print(largest([3, 9, 4]))
   |           ^
```

The body compiles now. The error moved to the call, which is where the bound
puts it. The message may surprise you, because `3 < 9` obviously works. But
integer comparison is a builtin rule, not an implementation of our `Ord` trait,
and the bound asks for the trait. Traits can be implemented for builtin types
as well as your own, so we can supply one. While we're at it, we'll give
`largest` an empty-list answer of `None` instead of trapping on `pop`, and
implement `Ord` for `Money`:

*Filename: src/main.au*

```aura
trait Ord[Rhs]:
    def lt(self, rhs: Rhs) -> bool
    def le(self, rhs: Rhs) -> bool
    def gt(self, rhs: Rhs) -> bool
    def ge(self, rhs: Rhs) -> bool

copy class Money:
    cents: int64

impl Display for Money:
    def display(self) -> str:
        return f"${self.cents // 100}.{self.cents % 100:02}"

impl Ord[Money] for Money:
    def lt(self, rhs: Money) -> bool:
        return self.cents < rhs.cents

    def le(self, rhs: Money) -> bool:
        return self.cents <= rhs.cents

    def gt(self, rhs: Money) -> bool:
        return self.cents > rhs.cents

    def ge(self, rhs: Money) -> bool:
        return self.cents >= rhs.cents

impl Ord[int64] for int64:
    def lt(self, rhs: int64) -> bool:
        return self < rhs

    def le(self, rhs: int64) -> bool:
        return self <= rhs

    def gt(self, rhs: int64) -> bool:
        return self > rhs

    def ge(self, rhs: int64) -> bool:
        return self >= rhs

def largest[T: Ord[T]](values: own list[T]) -> T | None:
    mut rest = values
    if rest.len() == 0:
        return None
    mut best = rest.pop()
    while rest.len() > 0:
        candidate = rest.pop()
        if candidate > best:
            best = candidate
    return best

def announce[T: Ord[T] + Display](values: own list[T]):
    winner = largest(values)
    if winner is None:
        print("nothing to compare")
    else:
        print(f"largest: {winner}")

def main():
    print(largest([3, 9, 4]))
    announce([Money(cents=1300), Money(cents=450), Money(cents=1700)])
    empty: list[Money] = []
    announce(empty)
```

<span class="caption">Listing 10-8: A generic `largest` bounded by `Ord`, used with integers and with `Money`</span>

```console
$ aura run src/main.au
9
largest: $17.00
nothing to compare
```

One function now serves both types, and the signature says exactly what it
needs. Inside `impl Ord[int64] for int64`, `self < rhs` uses the builtin
integer comparison, so there's no recursion. Builtin rules always take
precedence over trait dispatch.

`announce` shows the `+` form of bounds: it calls `largest`, which needs
`Ord[T]`, and prints the winner, which needs `Display`. Its bound states both.
The `Display` bound is a real requirement, not a formality. `announce([3, 9,
4])` is rejected with `type `int64` does not implement trait `Display``,
because integers print through a builtin rule rather than through a `Display`
implementation.

That's a fair amount of code to make integers comparable, and it's an honest
cost of Aura 0.11: the operator traits are a protocol the compiler recognizes,
not a library of ready-made implementations for the builtin types. In practice
you'll mostly write bounds like `Ord[T]` for your own domain types, where you
have to say what "larger" means anyway.

## Limits worth knowing

Traits on builtin types come with one rule. A trait method can't have the same
name as a builtin method of the type it's implemented for, because the builtin
would always win and your method would never run. The compiler rejects the
collision up front:

```aura check-fail:AU2006
trait Sized:
    def len(self) -> int64

impl Sized for str:
    def len(self) -> int64:
        return 0
```

```console
$ aura check src/main.au
error[AU2006]: trait method `len` collides with builtin method `str.len`
 --> src/main.au:5:5
  |
5 |     def len(self) -> int64:
  |     ^
  = help: rename the trait method; builtin methods cannot be shadowed by trait implementations
```

A few features you may know from other languages don't exist in Aura today:
trait objects and dynamic dispatch, associated types and constants, and `where`
clauses. Every trait call is resolved at compile time to one implementation.
When you need a list of mixed types, use a union, as Listing 10-6 did. The
[Manual](/manual/generics-and-traits) has the complete rules for
implementations, overlap, and dispatch.

There's one more kind of trait method we haven't used: a method with no `self`,
called on the type instead of on a value. It's how you write "make a `Money`
from this text", which brings us to converting between types.
