# Contracts and proofs

A test checks the inputs you thought of. When `test_discount` passes, we know
`apply_discount` works for the carts in that test, and we hope it works for
the rest. Most of the time that hope is well founded. But the bugs that reach
production tend to live in exactly the inputs nobody wrote down: a percentage
above 100, a price near the top of `int64`, an empty list.

Aura lets a function state what it needs and what it promises, as part of
its signature, and then `aura verify` can prove that those statements hold
for every input, not only the ones in your tests. In this section we'll add
contracts to the discount function from `checkout`, watch the verifier find
inputs that break it, and fix it until every check is proved.

## Stating what a function needs

Here is `apply_discount` with two new kinds of line at the top of its body:

*Filename: src/main.au*

```aura
def apply_discount(cents: int64, percent: int64) -> int64:
    requires 0 <= percent and percent <= 100
    ensures 0 <= result and result <= cents
    return cents - cents * percent // 100

def main():
    print(apply_discount(2500, 10))
    print(apply_discount(2500, 150))
```

<span class="caption">Listing 11-8: A discount with a `requires` and an `ensures` clause</span>

A `requires` clause is a condition the caller must meet: a percentage from 0
to 100. An `ensures` clause is a promise the function makes about its
`result`: the discounted price is never negative and never more than the
original. Both are ordinary `bool` expressions, and both are checked while
the program runs, so the second call stops the program at once:

```console
$ aura run src/main.au
2250
error[AU4009]: `requires` clause failed: `0 <= percent and percent <= 100`
 --> src/main.au:2:14
  |
2 |     requires 0 <= percent and percent <= 100
  |              ^
  = note: Aura call chain (innermost first): apply_discount at 1:1 -> main at 6:1
```

That's already better than an `assert` buried in the body, because the
condition is where every caller and every reader will look for it. The
details of contracts, including `old()` for values on entry and `spec def`
helpers, are in the [contracts manual page](/manual/contracts).

## Proving it

A failed `requires` at run time still needs a run that reaches it. Let's ask
the verifier instead. `aura verify` needs an SMT solver, a program that
answers logic questions; install Z3 with `brew install z3`, `apt install z3`,
or `pip install z3-solver`.

```console
$ aura verify src/main.au
error[AU5101]: cannot prove `ensures` clause `0 <= result and result <= cents`
 --> src/main.au:3:13
  |
3 |     ensures 0 <= result and result <= cents
  |             ^
  = note: in `apply_discount` (ensures check)
  = note: counterexample: cents = -1, percent = 1
error[AU5101]: cannot prove integer result in range
 --> src/main.au:4:20
  |
4 |     return cents - cents * percent // 100
  |                    ^
  = note: in `apply_discount` (overflow check)
  = note: counterexample: cents = 9223372036854775807, percent = 100
error[AU5101]: cannot prove `requires` of `apply_discount` holds: `0 <= percent and percent <= 100`
 --> src/main.au:8:5
  |
8 |     print(apply_discount(2500, 150))
  |     ^
  = note: in `main` (requires check)
verified src/main.au: 3 of 6 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Nothing ran. The verifier read every path through both functions and found
three checks that can fail, each with inputs that make it fail. The last one
we expected: `main` passes 150. The other two are real bugs that no test of
ours would have found. A negative price of `-1` cents breaks the promise that
the result is at least 0, and a price near the top of `int64` overflows when
it's multiplied by 100. Neither looks likely, but `cents` comes from a price
file, and a price file can say anything.

The fix is to say what a price is. Cart prices in `checkout` are never
negative and never more than ten billion dollars, so we require that, and we
fix the call in `main`:

```aura
def apply_discount(cents: int64, percent: int64) -> int64:
    requires 0 <= cents and cents <= 1_000_000_000_000
    requires 0 <= percent and percent <= 100
    ensures 0 <= result and result <= cents
    return cents - cents * percent // 100

def main():
    print(apply_discount(2500, 10))
    print(apply_discount(2500, 100))
```

<span class="caption">Listing 11-9: A discount whose checks are all proved</span>

```console
$ aura verify src/main.au
verified src/main.au: 8 of 8 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

Eight checks, all proved: both `requires` at both calls, the `ensures`, and
the arithmetic, which cannot overflow because `cents * percent` is at most a
hundred trillion. That is a statement about every possible input, which is
something no number of tests can make.

Notice how `main` was verified. The verifier didn't look inside
`apply_discount` to see what it does; it only proved the `requires` at each
call and then assumed the `ensures`. Each function is proved on its own, from
the contracts of the functions it calls. That keeps proofs fast, and it means
a contract is the whole agreement between a function and its callers: if a
caller needs to know something about the result, the callee has to promise
it.

## Loops need invariants

Totals are where overflow likes to hide. Here's a cart total that clamps each
price to a sane range before adding it:

```aura
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    mut total = 0
    for i in range(len(prices)):
        price = min(max(prices[i], 0), 1_000_000)
        total += price
    return total

def main():
    print(cart_total([1250, 499, 2000]))
```

Every price is at most a million cents and there are at most ten thousand of
them, so the total can't overflow. The verifier disagrees:

```console
$ aura verify src/main.au
error[AU5101]: cannot prove integer result in range
 --> src/main.au:6:9
  |
6 |         total += price
  |         ^
  = note: in `cart_total` (overflow check)
  = note: counterexample: len(prices) = 1
verified src/main.au: 3 of 4 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The verifier proves a loop one iteration at a time, so it doesn't know how
many times `total += price` has already run. It knows only what never changes
in the loop and what the loop says about itself. Our reasoning above used a
fact about `total` that we never wrote down: after `i` iterations it's at
most `i` million. Writing that fact as an `invariant` gives the verifier what
it needs:

```aura
def cart_total(prices: list[int64]) -> int64:
    requires len(prices) <= 10_000
    mut total = 0
    for i in range(len(prices)):
        invariant 0 <= total and total <= i * 1_000_000
        price = min(max(prices[i], 0), 1_000_000)
        total += price
    return total

def main():
    print(cart_total([1250, 499, 2000]))
```

<span class="caption">Listing 11-10: A loop invariant that bounds the running total</span>

```console
$ aura verify src/main.au
verified src/main.au: 6 of 6 checks proved in 2 functions (/usr/bin/z3 (Z3 version 5.3.3 - 64 bit))
```

The verifier proves the invariant twice: once on entry, where `total` is 0,
and once after an arbitrary iteration, assuming it held before. Then it can
use it inside the loop to prove the addition safe. The invariant is also
checked at run time, like every contract, so it documents the loop for the
next reader and guards it in every run.

## Proofs and run-time checks

A proof doesn't remove anything by default. Every contract and every bounds
check stays in the program, which is why a `requires` that the verifier
couldn't prove still stops a bad call with a clear diagnostic. When you want
the proofs to pay for themselves, `aura build --remove-proved-checks` builds
a native binary without the bounds, overflow, divisor, and `assert` checks
the verifier proves can never fail, and keeps every other check.

The verifier reads strings, dictionaries, and sets too, so it proves that
a `prices[item]` read finds its key when a `requires item in prices` says
so. Floats are IEEE-754 values, so a contract over them means what the
hardware computes, NaN included. It doesn't model everything yet: most
library calls are outside what it reads today, and a check that depends on them is reported
as `AU5104` and stays a run-time check. Quantifiers are modeled:
a contract such as `requires forall(p in prices, 0 <= p)` is proved at every
call and assumed inside the function. The [verification manual page](/manual/verification)
lists exactly what is modeled.

## Summary

You can now test Aura code with nothing but the `aura` command. A test is a
parameterless `test_` function that asserts what should be true, and the runner
reports each case by file and name, with the operands of a failed comparison
printed for you. A trap in the code under test fails the case with its full
diagnostic and call chain. You can narrow a run to a file, a directory, or any
case whose name contains a substring, set time limits, share setup and teardown
between cases, generate cases from a list, and produce JSON for CI, while the
exit status tells any script whether the suite passed. Where a test checks
the inputs you chose, a contract states what must hold for all of them, and
`aura verify` proves it or shows you an input that breaks it.

Along the way the tests did more than catch mistakes. They found a real bug in
`format_cents`, forced a decision about the average of an empty cart, and
pushed `load_prices` apart into a pure parser and a thin file reader. The
verifier found two more, in inputs nobody would have written a test for.
That's the usual effect of writing tests and contracts early, and it's the
approach we'll take in the next chapter. In
[Chapter 12](/book/ch12-00-log-analyzer) we'll put everything from the last
few chapters together and build a command-line log analyzer, starting from an
empty directory.
