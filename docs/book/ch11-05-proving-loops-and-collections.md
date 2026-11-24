# Proving loops and collections

The `cart_total` we proved in the last section had to defend itself. It
clamped every price into a sane range before adding it, because nothing
told it what the list held. That works, but it hides bad data: a price of
`-499` quietly becomes 0 and the customer gets a free mug. We'd rather say
that a cart with a negative price is not a cart, and have the verifier hold
every caller to it.

In this section we'll grow the `checkout` proofs from single integers to
lists and dictionaries. Each step starts from code that looks right, and
each time we'll read what `aura verify` says about it before we fix it.

## Saying something about every element

A `requires` clause can describe a whole list with a quantifier.
`forall(p in prices, condition)` is true when the condition holds for every
element, so we can state the range of a price once, in the signature, and
drop the clamp:

*Filename: src/main.au*

```aura
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    requires forall(p in prices, 0 <= p and p <= 1_000_000)
    ensures 0 <= result and result <= len(prices) * 1_000_000
    mut total = 0
    for i in range(len(prices)):
        invariant 0 <= total and total <= i * 1_000_000
        total += prices[i]
    return total

def main():
    print(cart_total([1250, 499, 2000]))
    print(cart_total([1250, -499, 2000]))
```

<span class="caption">Listing 11-11: A total that requires every price to be in range</span>

Inside the function the verifier assumes the `forall`, so `prices[i]` is
known to be between 0 and a million and the invariant from Listing 11-10
still carries the proof. At each call it has to prove the `forall` instead,
and the second call in `main` can't:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `requires` of `cart_total` holds: `forall(p in prices, 0 <= p and p <= 1_000_000)`
 --> src/main.au:13:5
   |
13 |     print(cart_total([1250, -499, 2000]))
   |     ^
  = note: in `main` (requires check)
verified src/main.au: 11 of 12 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The negative price is now an error at the place that produced it, before
the program runs. If we run it anyway, the same clause stops the program at
the same call, because a quantifier is checked at run time like any other
condition: it evaluates the condition for each element. Delete the second
call and all 9 remaining checks are proved.

We wrote the loop as `for i in range(len(prices))` and not
`for price in prices` on purpose. The verifier tracks a `range` loop's
counter exactly, so an invariant can mention `i`, and after the loop it
knows `i` reached `len(prices)`. That is how the `ensures` gets its
`len(prices) * 1_000_000`. A `for price in prices` loop gives the verifier
no position to hang a fact on, so when a proof needs to count, loop over
the indexes.

## Proving what a loop builds

A sale takes a percentage off every item. Let's write `discount_all`, which
builds the list of sale prices, and total the result:

```aura
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    requires forall(p in prices, 0 <= p and p <= 1_000_000)
    ensures 0 <= result and result <= len(prices) * 1_000_000
    mut total = 0
    for i in range(len(prices)):
        invariant 0 <= total and total <= i * 1_000_000
        total += prices[i]
    return total

def discount_all(prices: list[int64], percent: int64) -> list[int64]:
    requires 0 <= percent and percent <= 100
    requires forall(p in prices, 0 <= p and p <= 1_000_000)
    mut out: list[int64] = []
    for i in range(len(prices)):
        out.append(prices[i] - prices[i] * percent // 100)
    return out

def main():
    sale = discount_all([1250, 499, 2000], 10)
    print(sale)
    print(cart_total(sale))
```

The program prints `[1125, 450, 1800]` and `3375`, and every check inside
`discount_all` is proved. The call that follows it is not:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `requires` of `cart_total` holds: `forall(p in prices, 0 <= p and p <= 1_000_000)`
 --> src/main.au:22:5
   |
22 |     print(cart_total(sale))
   |     ^
  = note: in `main` (requires check)
error[AU5101]: cannot prove `requires` of `cart_total` holds: `len(prices) <= 10_000`
 --> src/main.au:22:5
   |
22 |     print(cart_total(sale))
   |     ^
  = note: in `main` (requires check)
verified src/main.au: 15 of 17 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

We can see that three prices go in and three smaller prices come out. The
verifier can't, because it never looks inside a function it calls. All
`main` knows about `sale` is what `discount_all` promises, and so far it
promises nothing: `sale` could be a list of any length holding anything.
This is the rule from the last section doing its work. What a caller needs
to know has to be in the contract.

So `discount_all` needs two `ensures` clauses, one for the length and one
for the elements. And because the list is built by a loop, the loop needs
invariants that say the same two things about the part built so far:

```aura
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    requires forall(p in prices, 0 <= p and p <= 1_000_000)
    ensures 0 <= result and result <= len(prices) * 1_000_000
    mut total = 0
    for i in range(len(prices)):
        invariant 0 <= total and total <= i * 1_000_000
        total += prices[i]
    return total

def discount_all(prices: list[int64], percent: int64) -> list[int64]:
    requires 0 <= percent and percent <= 100
    requires forall(p in prices, 0 <= p and p <= 1_000_000)
    ensures len(result) == len(prices)
    ensures forall(i in range(len(prices)), 0 <= result[i] and result[i] <= prices[i])
    mut out: list[int64] = []
    for i in range(len(prices)):
        invariant len(out) == i
        invariant forall(j in range(i), 0 <= out[j] and out[j] <= prices[j])
        out.append(prices[i] - prices[i] * percent // 100)
    return out

def main():
    sale = discount_all([1250, 499, 2000], 10)
    print(sale)
    print(cart_total(sale))
```

<span class="caption">Listing 11-12: A loop that builds a list, with invariants about the part built so far</span>

```console
$ aura verify src/main.au
verified src/main.au: 29 of 29 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Read the two invariants as a description of `out` after `i` trips around the
loop: it has `i` elements, and each of the first `i` is a sale price no
larger than the price it came from. Before the first trip `i` is 0 and both
are true of an empty list. One `append` later they're true for `i + 1`. When
the range runs out, `i` is `len(prices)`, and the invariants have become the
two `ensures` clauses word for word. If you delete the invariants, both
`ensures` fail, for the reason we met in the last section: the verifier
proves one iteration at a time and keeps only what the loop says about
itself.

Quantifying over `range(len(prices))` instead of over `prices` is what lets
a clause talk about the same position in two lists. That's the usual shape
for a function that maps one list to another.

With the promises written down, the proof of `main` goes through.
`discount_all` promises that each sale price is at most the original, the
originals are the literals in `main`, and so every element of `sale` is in
range for `cart_total`. Nobody had to test a cart with a 100% discount or
an empty one. They're covered.

## Proving that a loop ends

Bulk orders in `checkout` earn a bigger discount at each quantity threshold:
10 units, 50 units, 100 units. The thresholds are sorted, so we can find how
many of them an order has reached with a binary search:

```aura
def tiers_reached(thresholds: list[int64], quantity: int64) -> int64:
    ensures 0 <= result and result <= len(thresholds)
    mut lo = 0
    mut hi = len(thresholds)
    while lo < hi:
        invariant 0 <= lo and lo <= hi and hi <= len(thresholds)
        mid = lo + (hi - lo) // 2
        if thresholds[mid] <= quantity:
            lo = mid
        else:
            hi = mid
    return lo

def main():
    print(tiers_reached([10, 50, 100], 5))
    print(tiers_reached([10, 50, 100], 60))
```

The invariant keeps `lo` and `hi` inside the list, which is enough to prove
that `thresholds[mid]` is in range and that the result is a valid count:

```console
$ aura verify src/main.au
verified src/main.au: 7 of 7 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Seven of seven, and the function has a bug. Run it and the first call
prints `0`, and the second never returns. With a quantity of 60 the search
reaches `lo = 1` and `hi = 2`, computes `mid = 1`, sets `lo = mid`, and is
back where it started.

The proof wasn't wrong. Everything we stated is true: whenever
`tiers_reached` returns, its result is in range. We never claimed that it
returns. A `while` loop makes that claim with a `decreases` clause, which
names an integer that gets smaller on every trip and never goes below 0. A
loop with such a measure can't run forever. For a binary search the measure
is the width of the window, so we add one line under the invariant:

```aura fragment
        invariant 0 <= lo and lo <= hi and hi <= len(thresholds)
        decreases hi - lo
```

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `decreases` measure `hi - lo` gets smaller in every iteration
 --> src/main.au:7:19
  |
7 |         decreases hi - lo
  |                   ^
  = note: in `tiers_reached` (decreases check)
  = note: counterexample: len(thresholds) = 9223372036854775807, quantity = -9223372036854744211
verified src/main.au: 9 of 10 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Now the verifier refuses, and it found the problem without the second call
in `main`. The counterexample is less helpful than the ones we've read so
far. It lists the function's parameters, and the solver picked the largest
list `int64` allows, but the values that matter here are the locals `lo` and
`hi`, which it doesn't print. The message itself is the clue: some
iteration leaves `hi - lo` unchanged. Only one branch can do that, the one
that sets `lo = mid` when `mid` already equals `lo`. The fix is the classic
one:

```aura
def tiers_reached(thresholds: list[int64], quantity: int64) -> int64:
    ensures 0 <= result and result <= len(thresholds)
    mut lo = 0
    mut hi = len(thresholds)
    while lo < hi:
        invariant 0 <= lo and lo <= hi and hi <= len(thresholds)
        decreases hi - lo
        mid = lo + (hi - lo) // 2
        if thresholds[mid] <= quantity:
            lo = mid + 1
        else:
            hi = mid
    return lo

def main():
    print(tiers_reached([10, 50, 100], 5))
    print(tiers_reached([10, 50, 100], 60))
```

<span class="caption">Listing 11-13: A binary search that is proved to end</span>

```console
$ aura verify src/main.au
verified src/main.au: 11 of 11 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
$ aura run src/main.au
0
2
```

Like every contract, `decreases` is also checked while the program runs. If
you run the broken version with the clause in place, it stops at the stuck
iteration with `AU4009` and the message "`hi - lo` did not get smaller in
the last iteration", which is a much better afternoon than a program that
hangs.

A `for` loop always ends, so it takes no `decreases`. A function that calls
itself can state one at the top of its body, next to `requires`, and the
verifier proves that the measure is smaller at every recursive call. The
[contracts manual page](/manual/contracts#recursive-functions) shows that
form.

## Promising that something exists

`forall` has a partner. `exists(p in prices, condition)` is true when at
least one element meets the condition, and it's what a search needs to
state its result properly. Take the cheapest item in a cart. Saying the
result is no larger than any price is half the promise, because `0` would
satisfy it for any cart. The other half is that the result is one of the
prices:

```aura
def cheapest(prices: list[int64]) -> int64:
    requires len(prices) > 0
    ensures forall(p in prices, result <= p)
    ensures exists(p in prices, p == result)
    mut best = prices[0]
    for i in range(len(prices)):
        invariant forall(j in range(i), best <= prices[j])
        invariant exists(p in prices, p == best)
        if prices[i] < best:
            best = prices[i]
    return best

def main():
    print(cheapest([1250, 499, 2000]))
```

<span class="caption">Listing 11-14: A search whose result is proved to come from the list</span>

```console
$ aura verify src/main.au
verified src/main.au: 11 of 11 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The `requires len(prices) > 0` does two jobs. It proves that `prices[0]` is
in range, and it makes the `exists` possible at all, since an empty cart has
no cheapest item. The first invariant covers the prices seen so far, and the
second says `best` is always some element of the list, which is true of
`prices[0]` on entry and stays true each time `best` is replaced by
`prices[i]`.

## Dictionary keys

A list read can fail in one way, an index out of range. A dictionary read
can fail too, when the key isn't there, and the verifier treats it the same
way: every `prices[item]` is a check to prove. Here's a subtotal that looks
up each item of an order in a price table:

```aura
def subtotal(prices: dict[str, int64], items: list[str]) -> int64:
    mut total = 0
    for i in range(len(items)):
        total += prices[items[i]]
    return total

def main():
    prices = {"tea": 450, "mug": 1250}
    print(subtotal(prices, ["tea", "mug", "tea"]))
```

```console
$ aura verify src/main.au
error[AU5101]: cannot prove integer result in range
 --> src/main.au:4:9
  |
4 |         total += prices[items[i]]
  |         ^
  = note: in `subtotal` (overflow check)
  = note: counterexample: len(prices) = 1, len(items) = 2
error[AU5101]: cannot prove key is in the dict
 --> src/main.au:4:25
  |
4 |         total += prices[items[i]]
  |                         ^
  = note: in `subtotal` (key present check)
  = note: counterexample: len(prices) = 1, len(items) = 2
verified src/main.au: 2 of 4 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Two unproved checks, and by now we can guess both fixes. The key check
needs to know that every ordered item has a price, which is a `forall` over
`items`. The overflow check needs a bound on the values, which is a
`forall` over the table's keys, and the same invariant as before:

```aura
def subtotal(prices: dict[str, int64], items: list[str]) -> int64:
    requires len(items) <= 10_000
    requires forall(item in items, item in prices)
    requires forall(k in prices.keys(), 0 <= prices[k] and prices[k] <= 1_000_000)
    mut total = 0
    for i in range(len(items)):
        invariant 0 <= total and total <= i * 1_000_000
        total += prices[items[i]]
    return total

def main():
    prices = {"tea": 450, "mug": 1250}
    print(subtotal(prices, ["tea", "mug", "tea"]))
```

<span class="caption">Listing 11-15: Dictionary reads that are proved to find their keys</span>

```console
$ aura verify src/main.au
verified src/main.au: 11 of 11 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The verifier read the dictionary literal in `main`, so it knows which keys
`prices` holds and what their values are, and it proved all three `requires`
for the call. Add a line that orders something the shop doesn't sell,
`print(subtotal(prices, ["tea", "jam"]))`, and the verifier names the
clause that call breaks:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `requires` of `subtotal` holds: `forall(item in items, item in prices)`
 --> src/main.au:14:5
   |
14 |     print(subtotal(prices, ["tea", "jam"]))
   |     ^
  = note: in `main` (requires check)
verified src/main.au: 13 of 14 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

In a real `checkout` the order comes from a customer, so no literal will be
there for the verifier to read. The `requires` then tells us what the code
between the input and `subtotal` has to establish. Add `order_total` above
`main`; it checks an order before it totals it:

```aura
def subtotal(prices: dict[str, int64], items: list[str]) -> int64:
    requires len(items) <= 10_000
    requires forall(item in items, item in prices)
    requires forall(k in prices.keys(), 0 <= prices[k] and prices[k] <= 1_000_000)
    mut total = 0
    for i in range(len(items)):
        invariant 0 <= total and total <= i * 1_000_000
        total += prices[items[i]]
    return total

def order_total(prices: dict[str, int64], items: list[str]) -> int64 | None:
    requires forall(k in prices.keys(), 0 <= prices[k] and prices[k] <= 1_000_000)
    if len(items) > 10_000:
        return None
    for i in range(len(items)):
        invariant forall(j in range(i), items[j] in prices)
        if items[i] not in prices:
            return None
    return subtotal(prices, items)

def main():
    prices = {"tea": 450, "mug": 1250}
    print(order_total(prices, ["tea", "mug", "tea"]))
    print(order_total(prices, ["tea", "jam"]))
```

<span class="caption">Listing 11-16: Checking an order so that the call to `subtotal` is proved</span>

```console
$ aura verify src/main.au
verified src/main.au: 20 of 20 checks proved in 3 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
$ aura run src/main.au
2150
None
```

`order_total` asks nothing of `items`, so it accepts any order, including
the one with jam in it. The length test and the loop turn an unknown list
into one that meets `subtotal`'s contract, and the invariant records the
loop's progress: the first `i` items all have prices. This is the shape
most verified programs take. Input arrives with no guarantees, a small
function checks it and returns `None` or an error when it's bad, and
everything past that point is proved never to trap.

The verifier models dictionaries and sets whose keys are integers or
strings. The [verification manual page](/manual/verification#what-it-models)
lists the operations it reads.

Everything we've proved so far has been about functions and the values
passed between them. The next section moves the contracts onto types: a
class that keeps a promise about its fields for its whole life, and a trait
whose every implementation is held to the same contract.
