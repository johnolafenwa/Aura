# Trait objects

Listing 10-6 put books and coffees in one list by naming both types:
`list[Book | Coffee]`. That works while we know every type that will ever go
in the list. A shop's discounts are not like that. Today there is a percentage
off and a coupon, next month marketing invents a loyalty reward, and a
partner's module adds its own. Every new kind would mean editing the union,
and every function that takes the list.

What we want to say is "a list of anything that is a discount". Aura spells
that `list[dyn Discount]`. The type `dyn Discount` is a *trait object*: a
value of some type that implements `Discount`, where which type is decided
while the program runs. `dyn` is short for dynamic.

## A list of anything that implements a trait

Let's write the discounts. A discount has a name for the receipt and decides
how many cents to take off a subtotal:

```aura
trait Discount:
    def name(self) -> str
    def cut_cents(self, subtotal: int64) -> int64

class PercentOff:
    percent: int64

class Coupon:
    code: str
    cents: int64
    used: bool

impl Discount for PercentOff:
    def name(self) -> str:
        return f"{self.percent}% off"

    def cut_cents(self, subtotal: int64) -> int64:
        return subtotal * self.percent // 100

impl Discount for Coupon:
    def name(self) -> str:
        return f"coupon {self.code}"

    def cut_cents(self, subtotal: int64) -> int64:
        if self.used:
            return 0
        return min(self.cents, subtotal)
def checkout(subtotal: int64, discounts: list[dyn Discount]) -> int64:
    mut total = subtotal
    for discount in discounts:
        cut = discount.cut_cents(total)
        print(f"{discount.name()}: -{cut} cents")
        total -= cut
    return total

def main():
    discounts: list[dyn Discount] = [
        PercentOff(percent=10),
        Coupon(code="WELCOME", cents=250, used=false),
    ]
    print(f"to pay: {checkout(2000, discounts)} cents")
```

<span class="caption">Listing 10-10: One list holding two types of discount</span>

```console
$ aura run src/main.au
10% off: -200 cents
coupon WELCOME: -250 cents
to pay: 1550 cents
```

Nothing about the trait or the two `impl` blocks is new. The new part is the
type `list[dyn Discount]`, in `main` and in `checkout`. Inside the loop,
`discount.cut_cents(total)` runs `PercentOff`'s method for the first element
and `Coupon`'s for the second. The compiler can't pick the method ahead of
time as it does for a generic function, so the program looks at the type the
value holds and picks then.

Notice what `checkout` does not mention: `PercentOff` and `Coupon`. A module
written next year can define a `LoyaltyReward`, implement `Discount` for it,
and append one to the list. `checkout` runs it without being touched.

## Where a value becomes a trait object

In `main`, each element was written as an ordinary constructor call and ended
up as a `dyn Discount`. Aura converts a value to a trait object wherever the
code already says a trait object is expected: an annotated binding like the
one above, an argument for a `dyn Discount` parameter, a `return` from a
function that returns one, or a field of that type. The conversion moves the
value into the trait object. Nothing is copied.

Take the annotation away and there's nothing to say what the list should
hold, so the compiler falls back to its usual rule that list elements share
one type:

```aura check-fail:AU2002
trait Discount:
    def name(self) -> str
    def cut_cents(self, subtotal: int64) -> int64

class PercentOff:
    percent: int64

class Coupon:
    code: str
    cents: int64
    used: bool

impl Discount for PercentOff:
    def name(self) -> str:
        return f"{self.percent}% off"

    def cut_cents(self, subtotal: int64) -> int64:
        return subtotal * self.percent // 100

impl Discount for Coupon:
    def name(self) -> str:
        return f"coupon {self.code}"

    def cut_cents(self, subtotal: int64) -> int64:
        if self.used:
            return 0
        return min(self.cents, subtotal)
def main():
    discounts = [PercentOff(percent=10), Coupon(code="WELCOME", cents=250, used=false)]
    print(len(discounts))
```

```console
$ aura check src/main.au
error[AU2002]: list literal elements must all have type `PercentOff`, found `Coupon`
 --> src/main.au:30:42
   |
30 |     discounts = [PercentOff(percent=10), Coupon(code="WELCOME", cents=250, used=false)]
   |                                          ^
```

Aura could have guessed that we meant a list of trait objects, but a guess
like that would hide a real mistake just as often. Instead you say it. Either
annotate the binding, or convert one element with `as` and let the rest
follow:

```aura
trait Discount:
    def name(self) -> str
    def cut_cents(self, subtotal: int64) -> int64

class PercentOff:
    percent: int64

class Coupon:
    code: str
    cents: int64
    used: bool

impl Discount for PercentOff:
    def name(self) -> str:
        return f"{self.percent}% off"

    def cut_cents(self, subtotal: int64) -> int64:
        return subtotal * self.percent // 100

impl Discount for Coupon:
    def name(self) -> str:
        return f"coupon {self.code}"

    def cut_cents(self, subtotal: int64) -> int64:
        if self.used:
            return 0
        return min(self.cents, subtotal)
def main():
    discounts = [PercentOff(percent=10) as dyn Discount, Coupon(code="WELCOME", cents=250, used=false)]
    print(len(discounts))
```

This is the same `as` that converts numbers in
[the section on conversions](/book/ch10-03-conversions), and it follows the
same idea: nothing becomes a trait object behind your back.

## What a trait object gives up

A `dyn Discount` could hold any type, so the compiler lets you do only what
every discount can do, which is exactly what the trait declares. Reaching for
a field of the type you happen to know is inside is an error:

```aura check-fail:AU2002
trait Discount:
    def name(self) -> str
    def cut_cents(self, subtotal: int64) -> int64

class PercentOff:
    percent: int64

class Coupon:
    code: str
    cents: int64
    used: bool

impl Discount for PercentOff:
    def name(self) -> str:
        return f"{self.percent}% off"

    def cut_cents(self, subtotal: int64) -> int64:
        return subtotal * self.percent // 100

impl Discount for Coupon:
    def name(self) -> str:
        return f"coupon {self.code}"

    def cut_cents(self, subtotal: int64) -> int64:
        if self.used:
            return 0
        return min(self.cents, subtotal)
def main():
    discount: dyn Discount = PercentOff(percent=10)
    print(discount.percent)
```

```console
$ aura check src/main.au
error[AU2002]: `dyn Discount` has no field `percent`
 --> src/main.au:31:20
   |
31 |     print(discount.percent)
   |                    ^
  = note: a trait object exposes the methods of its trait, not the fields of the type it holds
  = help: add a method to the trait that returns the value
```

The help line is the usual fix: if every discount should be able to answer the
question, the question belongs in the trait.

For the same reason a trait object has no `==` and no `clone()`. Two
`dyn Discount` values might hold different types, and `Discount` says nothing
about comparing or copying. If you need either, add a method to the trait that
does it. A trait object is still an ordinary owned value in every other way:
you can move it, lend it to a function, store it in a class, and return it.

A few trait methods can't be called on a trait object at all. One call has to
make sense for whatever type is inside, so a method that has type parameters
of its own, or that mentions `Self` anywhere besides the receiver, is left
out. The trait can still declare such methods. They work on values whose type
is known and through a type parameter, and the compiler tells you when you
call one on a `dyn` value.

## Getting the type back

Sometimes one kind of value does need special treatment. A coupon has a code
the customer must type in, and no other discount does. `match` can ask a trait
object which type it holds, with the same `case Type as name:` arm that works
on a union:

```aura check-fail:AU2013
trait Discount:
    def name(self) -> str
    def cut_cents(self, subtotal: int64) -> int64

class PercentOff:
    percent: int64

class Coupon:
    code: str
    cents: int64
    used: bool

impl Discount for PercentOff:
    def name(self) -> str:
        return f"{self.percent}% off"

    def cut_cents(self, subtotal: int64) -> int64:
        return subtotal * self.percent // 100

impl Discount for Coupon:
    def name(self) -> str:
        return f"coupon {self.code}"

    def cut_cents(self, subtotal: int64) -> int64:
        if self.used:
            return 0
        return min(self.cents, subtotal)
def describe(discount: dyn Discount) -> str:
    match discount:
        case Coupon as coupon:
            return f"enter {coupon.code} at checkout"
```

```console
$ aura check src/main.au
error[AU2013]: match on `dyn Discount` needs a final `case _:`, because a trait object can hold any type that implements its trait
 --> src/main.au:30:5
   |
30 |     match discount:
   |     ^
  = help: add `case _:` after the type arms
```

A `match` on a union is complete when it has an arm for every member. A trait
object has no list of members, which was the point of using one, so the
compiler asks for a catch-all. With it, the function compiles, and inside the
`Coupon` arm `coupon` really is a `Coupon` with its fields:

```aura
trait Discount:
    def name(self) -> str
    def cut_cents(self, subtotal: int64) -> int64

class PercentOff:
    percent: int64

class Coupon:
    code: str
    cents: int64
    used: bool

impl Discount for PercentOff:
    def name(self) -> str:
        return f"{self.percent}% off"

    def cut_cents(self, subtotal: int64) -> int64:
        return subtotal * self.percent // 100

impl Discount for Coupon:
    def name(self) -> str:
        return f"coupon {self.code}"

    def cut_cents(self, subtotal: int64) -> int64:
        if self.used:
            return 0
        return min(self.cents, subtotal)
def describe(discount: dyn Discount) -> str:
    match discount:
        case Coupon as coupon:
            return f"enter {coupon.code} at checkout"
        case _:
            return discount.name()

def redeem(discounts: mut list[dyn Discount]):
    for discount in mut discounts:
        match mut discount:
            case Coupon as coupon:
                coupon.used = true
            case _:
                pass

def main():
    mut discounts: list[dyn Discount] = [
        PercentOff(percent=10),
        Coupon(code="WELCOME", cents=250, used=false),
    ]
    for discount in discounts:
        print(describe(discount))
    print(discounts[1].cut_cents(2000))
    redeem(discounts)
    print(discounts[1].cut_cents(2000))
```

<span class="caption">Listing 10-11: Matching on the type a trait object holds</span>

```console
$ aura run src/main.au
10% off
enter WELCOME at checkout
250
0
```

`match` follows the rules you know from [Chapter 6](/book/ch06-00-enums-and-match).
A plain `match` lends the value, so `describe` only reads the coupon.
`match mut` gives the arm a binding it can change, and the change lands in the
list, which is how `redeem` marks every coupon as used. `match own` moves the
value out of the trait object for good.

Reach for a type arm sparingly. Each one is a place that knows about a
specific type, and the more of them you write, the less open your list is.
When most of your code wants to match, a union or an enum is the better fit.

## Generic, union, or trait object

We now have three ways to write code over several types, and they answer
different questions.

A type parameter, `def total[T: Priced](items: list[T])`, is one type chosen
at each call. Every element of the list has that same type, the compiler
builds a copy of the function for it, and calls are direct. Use it when the
caller knows the type.

A union, `list[Book | Coffee]`, mixes types from a fixed set. The compiler
knows every member, so it can check that a `match` handles all of them, and
values still compare and clone when their members do. Use it when you can
list the types.

A trait object, `list[dyn Priced]`, mixes types from an open set. New types
join without any change to the code that uses the list, and in exchange you
get only the trait's methods. Use it for plugins, handlers, strategies, and
anything else where the code that holds the values shouldn't know what they
are.

The cost of a trait object is a small amount of work per call to find the
right method. In our measurements a call through a `dyn` value in a loop is
within three times the cost of the same call through Rust's `Box<dyn Trait>`
and several times faster than a method call in Python, so it is rarely the
thing to optimize.

Trait objects cross task boundaries only when the trait promises that every
implementing type is safe to hand to another task. You write that promise as
`trait Job: Transfer:`, and
[Chapter 15](/book/ch15-01-tasks-and-task-groups) shows it at work. A
`dyn Iterator[T]` is a `for` source, so a function can return one of several
iterator types behind a single return type. The
[Manual](/manual/generics-and-traits#trait-objects) has the complete rules.

## Summary

Generics let you write a function, class, or enum once and use it with many
types. Type parameters are checked once, for every possible type, so the
ownership modes and operations in a generic body have to make sense for all of
them: `own T` for values the body keeps, and a trait bound for any method or
operator it calls. Traits name behavior, implementations promise it explicitly,
and bounds put the requirement in the signature where callers can read it. A
few traits connect to the language itself: `Display` controls printing, `Add`,
`Ord`, and friends give your types operators, `From` lets `try` convert
errors, and `Iterator` and `IntoIterator` decide what a `for` loop does with
your types.

Conversions follow the same philosophy of saying what you mean. `as` converts
numbers exactly or traps, `.to_float()` rounds on purpose, parsing text returns
a `Result`, and `str()` and f-strings render anything. Nothing converts behind
your back.

Generators let you write a lazy sequence as an ordinary loop with `yield`.
Nothing runs until a loop asks for the next item, which makes endless sequences
safe to zip with finite ones, and a generator that stops early still runs its
cleanup.

Trait objects cover the case generics and unions can't: values of types the
code has never heard of. `dyn Trait` holds any implementing type, converts
where the type is written or with `as`, offers the trait's methods, and gives
the type back through `match` with a catch-all.

With error handling and generics in hand, you're ready to make sure your code
does what you think it does. [Chapter 11](/book/ch11-00-testing) introduces
`aura test`, and we'll use it to find a real bug in some code that looks
correct.
