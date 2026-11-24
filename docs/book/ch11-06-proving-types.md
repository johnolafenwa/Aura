# Proving types

The contracts so far have belonged to functions. Each one describes a
single call: what goes in, what comes out. Some rules aren't about a call.
A gift card's balance is never negative, whoever touches the card and
however many times. A discount never raises a price, whichever kind of
discount it is. Rules like these belong to a type, and Aura lets you state
them there, once, and proves that every method and every implementation
keeps them.

In this section we'll give `checkout` a gift card and a family of
discounts, and meet the remaining tools the verifier offers along the way.
Then we'll look at how to read its reports, what it can't prove yet, and
how to make the proofs part of your daily loop.

## A class that keeps its promise

A gift card has a balance and a limit on how much it can hold. Between any
two operations the balance is somewhere from 0 to the limit. We state that
with an `invariant` line in the class body, where `self` names the object:

*Filename: src/main.au*

```aura
class GiftCard:
    balance: int64
    limit: int64 = 50_000
    invariant 0 <= self.balance and self.balance <= self.limit

    public def top_up(mut self, cents: int64):
        requires cents > 0
        self.balance += cents

    public def charge(mut self, cents: int64):
        requires 0 < cents and cents <= self.balance
        self.balance -= cents

def main():
    mut card = GiftCard(balance=2_000)
    card.top_up(500)
    card.charge(1_250)
    print(card.balance)
```

<span class="caption">Listing 11-17: A gift card with a class invariant, and a method that doesn't yet keep it</span>

The program runs and prints `1250`. The verifier is less satisfied:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove integer result in range
 --> src/main.au:8:9
  |
8 |         self.balance += cents
  |         ^
  = note: in `GiftCard.top_up` (overflow check)
  = note: counterexample: self.balance = 1, self.limit = 1, cents = 9223372036854775807
error[AU5101]: cannot prove class invariant holds: `0 <= self.balance and self.balance <= self.limit`
 --> src/main.au:8:9
  |
8 |         self.balance += cents
  |         ^
  = note: in `GiftCard.top_up` (class invariant check)
  = note: counterexample: self.balance = 0, self.limit = 0, cents = 1
error[AU5101]: cannot prove `requires` of `GiftCard.charge` holds: `0 < cents and cents <= self.balance`
 --> src/main.au:17:5
   |
17 |     card.charge(1_250)
   |     ^
  = note: in `main` (requires check)
verified src/main.au: 8 of 11 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Start with what was proved, because the invariant already earned its keep.
`charge` has no complaints. On entry to a `public` method the verifier
assumes the invariant, and on the way out it proves it again. `charge`
requires `cents <= self.balance`, so the subtraction can't go below 0, and
a balance that was at most the limit still is. The construction in `main`
was checked too: `GiftCard(balance=2_000)` is proved to satisfy the
invariant, and `GiftCard(balance=-5)` would be reported there.

`top_up` is the method that breaks the promise. The second error says so
directly, with a card that is already full (balance 0, limit 0) taking one
more cent. The first error is the same mistake at a larger scale: nothing
stops a top-up big enough to overflow `int64`. Both go away when `top_up`
requires that the money fits.

The third error is in `main`, and it's the lesson of the last two sections
again. After `card.top_up(500)`, what `main` knows about the balance is
what `top_up` promises, and it promises nothing, so the verifier can't tell
whether 1,250 cents are there to charge.

## What a method changed: `old`

To promise something about a method that modifies `self`, an `ensures`
clause has to compare the object after the call with the object before it.
Inside `ensures`, `old(expr)` is the value `expr` had on entry. Here is the
card with all three problems fixed:

```aura
class GiftCard:
    balance: int64
    limit: int64 = 50_000
    invariant 0 <= self.balance and self.balance <= self.limit

    spec def room(self) -> int64:
        return self.limit - self.balance

    public def top_up(mut self, cents: int64):
        requires 0 < cents and cents <= self.room()
        ensures self.balance == old(self.balance) + cents
        self.balance += cents

    public def charge(mut self, cents: int64):
        requires 0 < cents and cents <= self.balance
        ensures self.balance == old(self.balance) - cents
        self.balance -= cents

def main():
    mut card = GiftCard(balance=2_000)
    card.top_up(500)
    card.charge(1_250)
    print(card.balance)
    print(card.room())
```

<span class="caption">Listing 11-18: Methods that say what they change, and a `spec def` that names a condition</span>

```console
$ aura verify src/main.au
verified src/main.au: 17 of 17 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
$ aura run src/main.au
1250
48750
```

Each method now says exactly what it does to the balance, so `main` can
follow the card through both calls: 2,000, then 2,500, then 1,250, with the
charge proved to be covered. `old` takes a parameter, `self`, or a field or
element of one, so to talk about a list's earlier length you write
`len(old(self.items))`, not `old(len(self.items))`.

The other new thing is `room`. A `spec def` is a helper written for
contracts: its body is a single `return` of a pure expression, and clauses
may call it where they couldn't call an ordinary method. It gives a name to
a condition that would otherwise be repeated, and when the verifier meets
`self.room()` it reads the expression behind it. A `spec def` method is
also a normal method, which is why `main` can print `card.room()`.

There's one more gap, and it's easy to walk into. Add a second
`card.top_up(500)` after the charge:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `requires` of `GiftCard.top_up` holds: `0 < cents and cents <= self.room()`
 --> src/main.au:24:5
   |
24 |     card.top_up(500)
   |     ^
  = note: in `main` (requires check)
verified src/main.au: 18 of 19 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

We know the card has room: it holds 1,250 of a possible 50,000. The
verifier knows the balance is 1,250, but it no longer knows the limit. A
method that takes `mut self` may change any field, so after the call the
caller knows only what the `ensures` clauses say, and ours say nothing
about `limit`. The first `top_up` was fine because the card had just been
built with its default limit. The fix is to promise that the limit stays
put, in both methods:

```aura fragment
        ensures self.balance == old(self.balance) + cents
        ensures self.limit == old(self.limit)
```

With that line in `top_up` and `charge`, all 21 checks are proved. It feels
like stating the obvious, and it is, but it's the same rule as everywhere
else in this chapter: the contract is the whole agreement. The method body
is free to change tomorrow, and callers that were proved against the
contract stay proved.

## Facts for the verifier only: `ghost`

Let's use the card. `pay` walks a list of prices, charges each one the card
can cover, and returns how many it paid for. We'd like to promise that
paying never adds money to the card:

```aura
class GiftCard:
    balance: int64
    limit: int64 = 50_000
    invariant 0 <= self.balance and self.balance <= self.limit

    spec def room(self) -> int64:
        return self.limit - self.balance

    public def top_up(mut self, cents: int64):
        requires 0 < cents and cents <= self.room()
        ensures self.balance == old(self.balance) + cents
        ensures self.limit == old(self.limit)
        self.balance += cents

    public def charge(mut self, cents: int64):
        requires 0 < cents and cents <= self.balance
        ensures self.balance == old(self.balance) - cents
        ensures self.limit == old(self.limit)
        self.balance -= cents

def pay(card: mut GiftCard, prices: list[int64]) -> int64:
    """Charges each price the card can cover and returns how many it paid."""
    requires forall(p in prices, 0 < p)
    ensures 0 <= result and result <= len(prices)
    ensures card.balance <= old(card.balance)
    ghost start = card.balance
    mut paid = 0
    for i in range(len(prices)):
        invariant 0 <= paid and paid <= i
        invariant 0 <= card.balance and card.balance <= card.limit
        invariant card.balance <= start
        if prices[i] <= card.balance:
            card.charge(prices[i])
            paid += 1
    return paid

def main():
    mut card = GiftCard(balance=2_000)
    print(pay(card, [1_250, 499, 2_000]))
    print(card.balance)
```

<span class="caption">Listing 11-19: Paying from a gift card, with a ghost local that remembers the starting balance</span>

```console
$ aura verify src/main.au
verified src/main.au: 28 of 28 checks proved in 4 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
$ aura run src/main.au
2
251
```

The `ensures` uses `old(card.balance)`, but `old` exists only in `ensures`,
and the loop needs an invariant that mentions the starting balance too.
That's what `ghost start = card.balance` is for. A `ghost` local holds a
value that only contracts read. The verifier treats it like any other
local, and the running program never computes it, so it costs nothing.
Ordinary code can't read a ghost local; the compiler reports `AU2048` if it
tries, which keeps proof bookkeeping from leaking into what the program
does.

The second invariant restates the class invariant for `card`, and it has to
be there. A loop forgets everything about the values its body changes, and
a card that `charge` modifies is one of them, so without that line the
verifier reports that it "cannot prove class invariant holds before
`GiftCard.charge`". The class invariant is assumed when a function starts
and proved when an object crosses a function boundary. In between, across a
loop, you carry it yourself.

## One contract for every implementation

`checkout` has more than one kind of discount: a percentage off, a flat
amount off. They share a trait, and the trait is where the rule about
discounts belongs. A trait method can carry `requires` and `ensures`
clauses with no body, and every implementation inherits them:

```aura
trait Discount:
    def apply(self, cents: int64) -> int64:
        requires 0 <= cents and cents <= 1_000_000
        ensures 0 <= result and result <= cents

class PercentOff:
    percent: int64

class FlatOff:
    amount: int64

impl Discount for PercentOff:
    def apply(self, cents: int64) -> int64:
        percent = min(max(self.percent, 0), 100)
        return cents - cents * percent // 100

impl Discount for FlatOff:
    def apply(self, cents: int64) -> int64:
        amount = min(max(self.amount, 0), 1_000_000)
        return cents - amount

def best_price(deals: list[dyn Discount], cents: int64) -> int64:
    requires 0 <= cents and cents <= 1_000_000
    ensures 0 <= result and result <= cents
    mut best = cents
    for deal in deals:
        invariant 0 <= best and best <= cents
        best = min(best, deal.apply(cents))
    return best

def main():
    deals: list[dyn Discount] = [PercentOff(percent=10), FlatOff(amount=300)]
    print(best_price(deals, 2_500))
    print(best_price(deals, 200))
```

<span class="caption">Listing 11-20: A trait whose contract every discount must keep</span>

`best_price` takes a list of `dyn Discount`, so it can't know which
implementations are in it. It doesn't need to. The call `deal.apply(cents)`
is proved against the trait's contract: the `requires` holds because
`best_price` requires the same range, and the `ensures` says the result is
between 0 and `cents`, which keeps the loop invariant. The other side of
that bargain is that each `impl` must be proved to keep the contract, and
one of ours doesn't:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `ensures` clause `0 <= result and result <= cents`
 --> src/main.au:4:17
  |
4 |         ensures 0 <= result and result <= cents
  |                 ^
  = note: in `Discount for FlatOff.apply` (ensures check)
  = note: counterexample: self.amount = 1000001, cents = 0
verified src/main.au: 10 of 11 checks proved in 4 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

A flat discount larger than the price takes the price below zero. The
second call in `main` does exactly that, with 300 cents off a 200-cent
item, and running the program stops there with `AU4009`. The fix is to cap
the amount at the price, by changing one line of `FlatOff.apply`:

```aura fragment
        amount = min(max(self.amount, 0), cents)
```

```console
$ aura verify src/main.au
verified src/main.au: 11 of 11 checks proved in 4 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
$ aura run src/main.au
2200
0
```

Anyone who adds a third discount next year gets the same treatment. Their
`impl` won't verify until it keeps the promise that `best_price` and every
other caller already rely on.

You may wonder why both implementations clamp their field with `min` and
`max` when a class invariant such as `invariant 0 <= self.percent` would say
it better. Today the verifier assumes a class invariant only inside the
class's own `public def` methods, not inside the methods of an `impl` block,
so an implementation has to establish what it needs about `self` in its own
body. An implementation may also state its own contract: it keeps every
`ensures` of the trait and may add more, and it may replace the trait's
`requires` with weaker ones. The
[contracts manual page](/manual/contracts#trait-methods) has the rules.

## When the solver needs a hint: `lemma def`

Every proof so far went through as soon as the contracts said enough. Now
and then the contracts say enough and the solver still can't find the
proof. `checkout` has a reward scheme where a customer's first order in a
month earns 1 point, the second 2, and so on, so after `orders` orders they
hold `orders * (orders + 1) // 2` points. We'd like to promise that the
division is exact:

```aura
def points_after(orders: int64) -> int64:
    requires 0 <= orders and orders <= 1000
    ensures result * 2 == orders * (orders + 1)
    return orders * (orders + 1) // 2

def main():
    print(points_after(10))
```

```console
$ aura verify src/main.au
error[AU5102]: the solver could not decide `ensures` clause `result * 2 == orders * (orders + 1)`
 --> src/main.au:3:13
  |
3 |     ensures result * 2 == orders * (orders + 1)
  |             ^
  = note: in `points_after` (ensures check)
  = help: raise the limit with `--timeout <ms>`, or add a `requires`, `invariant`, or `assert` that narrows the case
verified src/main.au: 7 of 8 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

This is a new code. `AU5101` meant "this can fail, and here's how".
`AU5102` means the solver ran for its full ten seconds and gave up without
an answer either way, which is why there's no counterexample. The clause is
true. It depends on the product of two neighboring numbers being even, and
solvers are weak at facts about multiplying two unknowns.

A person proves that fact by induction: it holds for 0, and if it holds for
`n - 1` it holds for `n`. A `lemma def` lets us write that argument down:

```aura
lemma def product_is_even(n: int64):
    requires 0 <= n and n <= 1000
    decreases n
    ensures n * (n + 1) % 2 == 0
    if n > 0:
        product_is_even(n - 1)

def points_after(orders: int64) -> int64:
    requires 0 <= orders and orders <= 1000
    ensures result * 2 == orders * (orders + 1)
    product_is_even(orders)
    return orders * (orders + 1) // 2

def main():
    print(points_after(10))
```

<span class="caption">Listing 11-21: A lemma, proved by induction, that the solver could not find alone</span>

```console
$ aura verify src/main.au
verified src/main.au: 17 of 17 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

A lemma is a function with no result whose `ensures` states a fact and
whose body shows why it holds. The body here is the induction: for `n` above
0 it calls itself on `n - 1`, which hands the solver the fact for the
smaller case, and the step from there to `n` is easy. The `decreases n` is
what makes the argument sound, since it proves the chain of calls reaches 0.
The verifier proves the lemma once, and then every call to it, like the one
at the top of `points_after`, proves the lemma's `requires` and gets its
`ensures` as a known fact.

A lemma call written as a statement is erased when the program runs, so the
recursion costs nothing. You won't need lemmas often. When you do, the sign
is an `AU5102` on a clause you can prove on paper.

## Reading a report

`aura verify` exits with status 0 only when every check is proved, so it
drops into CI next to `aura test`. Each check it couldn't prove gets one
diagnostic, and there are four codes.

`AU5101` is the one we've read all chapter: the check can fail, and the
counterexample gives parameter values that make it fail. Read the
counterexample as a test case the solver wrote for you. It shows parameters
and fields on entry, not locals, and it tends to pick extreme values, so
ask what is special about these inputs, not whether anyone would pass them.
The answer is a real bug, or a fact you know and haven't stated. For the
second kind, state it: as a `requires` if callers must provide it, as an
`invariant` if a loop maintains it, or as an `ensures` on the function you
called.

`AU5102` means the solver gave up, as we saw with the reward points. Raise
the limit with `--timeout`, give the solver a stepping stone with an
`assert`, or write a lemma.

`AU5103` means no solver could be started, and the fix is to install Z3 or
point `AURA_SMT_SOLVER` at one.

`AU5104` means the check depends on an operation the verifier doesn't read
yet. A left shift is one:

```aura
def bulk_units(packs: int64) -> int64:
    requires 0 <= packs and packs <= 10
    return 1 << packs

def main():
    print(bulk_units(3))
```

```console
$ aura verify src/main.au
error[AU5104]: cannot check operation outside the verified subset: `Shl` is not modeled yet
 --> src/main.au:3:12
  |
3 |     return 1 << packs
  |            ^
  = note: in `bulk_units` (overflow check)
verified src/main.au: 1 of 2 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Nothing is wrong with the code. The check is not proved, and it stays a
run-time check as it always was.

## What the verifier doesn't model yet

Aura is a preview, and the verifier covers less of the language than the
compiler does. It reasons exactly about integers of every width, `bool`,
floats, strings, lists, dictionaries and sets with integer or string keys,
tuples, classes, enums, and unions. It helps to know where the edges are
before you run into them.

The edge you'll meet first is the unknown value. When the verifier doesn't
model an operation, it treats the result as a value it knows nothing about,
and a check that depends on that value is often reported as `AU5101` with a
counterexample, not as `AU5104`. Here is the total from this chapter
written with the built-in `sum`:

```aura
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    requires forall(p in prices, 0 <= p and p <= 1_000_000)
    ensures result >= 0
    return sum(prices)

def main():
    print(cart_total([1250, 499, 2000]))
```

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `ensures` clause `result >= 0`
 --> src/main.au:4:13
  |
4 |     ensures result >= 0
  |             ^
  = note: in `cart_total` (ensures check)
  = note: counterexample: len(prices) = 540
error[AU5101]: cannot prove integer result in range
 --> src/main.au:5:12
  |
5 |     return sum(prices)
  |            ^
  = note: in `cart_total` (overflow check)
  = note: counterexample: len(prices) = 541
verified src/main.au: 2 of 4 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The sum of non-negative prices is not negative, and this function is
correct. The verifier doesn't know what `sum` computes, so to it `result`
could be anything. A counterexample that makes no sense, on a line that
calls into the library, usually means this and not a bug. The cure is the
loop from Listing 11-11, whose invariant the verifier can follow.

These are the unknown values and unmodeled features to expect today:

- The results of comprehensions, slices such as `xs[0:2]`, `sum(xs)`,
  `xs.sort()`, `k in xs` on a list, f-strings, `text.split(...)` and most
  other `str` methods, and most library calls. Of strings, the verifier
  reads `len`, `+`, comparisons, `contains`, `starts_with`, and `ends_with`.
- The inner lengths of nested lists, so `grid[r][c]` doesn't prove, and of
  lists stored as dictionary values.
- Indexes into a `Span[T]`.
- The bit operations `&`, `|`, and `^`, and the `saturating_` and
  `wrapping_` integer methods. `<<` is reported as `AU5104`.
- Lambdas and local `def` functions. Their bodies are checked with captured
  values unknown, and callers learn nothing about what they return.
  [Chapter 13](/book/ch13-01-closures) shows the alternative that does
  verify.
- Generators. A generator function's `requires` is not assumed in its body,
  and the values it yields carry no contract.
- Class invariants inside `impl` methods, as we saw with the discounts.
- The result of `control.recover`, and the contents of queues, mutexes, and
  atomics, which other tasks can change. Starting a task is modeled;
  [Chapter 15](/book/ch15-01-tasks-and-task-groups) shows it.
- Tensor shapes.

Two more things are about how the tool works. `aura verify` takes one file
and follows that file's imports, so a project is verified through its entry
file or one module at a time. And a `requires` that can never be true
proves everything after it: a function that requires `a > b and b > a` has
no valid inputs, so every check in it is proved, silently. If a function
proves with suspiciously little effort, check that a call to it from `main`
proves too.

None of this weakens what a proof means. A check counts as proved only when
the solver shows that no input breaks it, and an unproved check stays in the
program as a run-time check. The limits decide how much you can prove. They
don't decide whether a proof can be trusted. The
[verification manual page](/manual/verification#what-it-models) keeps the
exact list of what is modeled.

## Proofs in your daily loop

Proofs are only useful if running them is cheap, so `aura verify` keeps a
proof cache. Run it twice on Listing 11-19 and the second summary line says
where the answers came from:

```console
$ aura verify src/main.au
verified src/main.au: 28 of 28 checks proved in 4 functions, 4 from the proof cache (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

A function whose checks and the contracts they rely on are unchanged is
answered from the cache without starting the solver, so after an edit only
the functions you touched are solved again. The cache lives in
`~/.cache/aura/verify`, or under `AURA_CACHE_DIR` when that is set, and
`--no-cache` bypasses it. While you work on one function,
`aura verify --function pay src/main.au` checks only the functions whose
name contains `pay`.

In VS Code you don't run the command at all. The Aura extension runs
`aura verify` each time an Aura file is opened or saved. A check it can't
prove appears as a diagnostic on its line, counterexample included, next to
the compiler's own errors, and each line whose checks are all proved ends
with a `✓ proved` hint. Writing a contract and watching the hint appear
becomes part of typing the function. If you'd rather run it by hand, set
`aura.verifyOnSave` to `false`.

## Letting proofs pay for themselves

A proof doesn't change the program by default. Every bounds check, overflow
check, and contract stays in and runs. Once a check is proved, though,
running it is wasted work, and `aura build --remove-proved-checks` builds a
native binary without the ones the verifier proved. Here it is on the gift
card program from Listing 11-19:

```console
$ aura build --remove-proved-checks -o checkout src/main.au
aura: 16 checks proved and removed (2 bounds, 6 arithmetic, 8 assertion)
aura: built `checkout` with direct backend
$ ./checkout
2
251
```

The flag runs the verifier first, then leaves out the list bounds checks,
the overflow and zero-divisor checks, and the `assert`, `ensures`,
`invariant`, and `decreases` checks that were proved on every path. A
function's `requires` always stays, because code the verifier didn't see
may call it. Anything unproved stays too, so the flag removes only what a
proof covers.

It's off by default, and the reason is trust. Removing a check means the
binary relies on the verifier and the solver being right, where before they
were only advisers. The [verification manual page](/manual/verification#trust)
explains what is removed and what the flag takes on trust.

## Summary

You can now check Aura code in two ways with nothing but the `aura`
command. A test is a parameterless `test_` function that asserts what should
be true, and the runner reports each case by file and name, with the
operands of a failed comparison printed for you. You can narrow a run to a
file, a directory, or any case whose name contains a substring, set time
limits, share setup and teardown between cases, generate cases from a list,
and produce JSON for CI.

A contract states what must hold for every input. `requires` and `ensures`
describe a function, `invariant` and `decreases` describe a loop, a class
`invariant` describes every object of a type, and a trait method's contract
binds every implementation. `aura verify` proves them, together with the
arithmetic, indexes, and dictionary reads around them, or shows you an input
that breaks one. You know how to carry a fact through a loop, how to say
what a method changed with `old`, when to reach for `spec def`, `ghost`, and
`lemma def`, and where the verifier's knowledge ends.

Along the way both halves did more than catch mistakes. The tests found a
real bug in `format_cents` and pushed `load_prices` apart into a pure parser
and a thin file reader. The verifier found an overflow, a negative price, a
binary search that never returns, and a discount that went below zero, none
of which we'd written a test for. Stating what a function needs also kept
improving its design. That's the usual effect of writing tests and
contracts early, and it's the approach we'll take in the next chapter. In
[Chapter 12](/book/ch12-00-log-analyzer) we'll put everything from the last
few chapters together and build a command-line log analyzer, starting from
an empty directory.
