# How to write tests

Tests live in a package, so we begin with an empty one. `aura new` creates it,
and as you saw in [Chapter 1](/book/ch01-03-hello-packages), it already
includes a place for tests:

```console
$ aura new checkout
created `checkout`
$ cd checkout
$ find . -type f | sort
./.gitignore
./Aura.toml
./src/main.au
./tests/smoke.au
```

Open `tests/smoke.au`:

*Filename: tests/smoke.au*

```aura
def main() -> int32:
    return 0
```

It's the smallest test there is: a program that exits with status 0. Run the package's tests:

```console
$ aura test
ok tests/smoke.au
1 passed; 0 failed
```

With no arguments, `aura test` looks through the `tests/` directory for `.au`
files and runs each one. `smoke.au` has no test functions in it, so the runner
treats the whole file as one test case and runs its `main`. It passes if `main`
returns 0 and fails if `main` returns anything else or traps. That makes the
file a smoke test, a quick check that the package builds and starts. You'll
also notice a new `Aura.lock` file next to `Aura.toml`; package commands such
as `aura run` and `aura test` write it to record the package's resolved
dependencies.

File-level cases are useful for end-to-end scripts, but most tests are smaller
than a whole program. For those, Aura has test functions.

## The code we'll test

Our package will total a shopping cart. Prices are in cents, as an `int64`,
because floating-point money accumulates rounding errors. Create `src/cart.au`
with one function:

*Filename: src/cart.au*

```aura
public def subtotal(prices: list[int64]) -> int64:
    mut total = 0
    for price in prices:
        total += price
    return total
```

<span class="caption">Listing 11-1: The first function of the `cart` module</span>

The function is `public` because the tests live in a different module and can
only import public names, as [Chapter 7](/book/ch07-00-modules-and-packages)
explained.

## Your first test function

Now create `tests/cart_test.au`:

*Filename: tests/cart_test.au*

```aura fragment
from cart import subtotal

def test_adds_prices():
    assert subtotal([250, 125]) == 375

def test_empty_cart_is_zero():
    assert subtotal([]) == 0
```

<span class="caption">Listing 11-2: Two test functions for `subtotal`</span>

The file imports `subtotal` from the `cart` module, the same way any other
module in the package would. Test files under `tests/` are part of the package,
so `import` and `from ... import` resolve against `src/` exactly as they do for
`src/main.au`.

Each test is a function that follows three rules. Its name starts with `test_`.
It takes no parameters. It returns `None`, which is what a function with no
`->` annotation returns. Inside, it calls the code under test and asserts what
should be true about the result. If every assertion holds, the function returns
normally and the test passes. If any assertion fails, the test traps, and a
trapped test is a failed test.

```console
$ aura test
ok tests/cart_test.au::test_adds_prices
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/smoke.au
3 passed; 0 failed
```

Each test function is reported as its own case, named by its file and its
function joined with `::`. Files run in path order, and the functions in a file
run in the order they're written.

## Making sure a test can fail

A test that has never failed hasn't proved much. It might not be testing what
you think, or it might not be running at all. So before trusting
`test_adds_prices`, let's break `subtotal` on purpose and watch the test catch
it. Change `+=` to `=` in the loop:

*Filename: src/cart.au*

```aura
public def subtotal(prices: list[int64]) -> int64:
    mut total = 0
    for price in prices:
        total = price
    return total
```

The function now returns the last price instead of the sum. Run the tests again:

```console
$ aura test
FAILED tests/cart_test.au::test_adds_prices
error[AU4001]: assertion failed
 --> tests/cart_test.au:4:5
  |
4 |     assert subtotal([250, 125]) == 375
  |     ^
  = note: left = 125
  = note: right = 375
  = note: Aura call chain (innermost first): test_adds_prices at 3:1
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/smoke.au
2 passed; 1 failed
```

The failing case is marked `FAILED` and followed by the assertion's diagnostic.
Two lines in it do most of the work: `left = 125` is what `subtotal` returned,
and `right = 375` is what we expected. Because the assertion's condition is a
single `==` comparison, Aura captured both operands when the comparison failed
and printed them. You don't need to add a `print` and run again to find out
what the function actually returned.

Notice too that `test_empty_cart_is_zero` still passed. The broken function
returns 0 for an empty list, the same as the correct one. That's a useful
reminder that each test checks one specific behavior, and a suite needs enough
cases to pin the function down. Change `=` back to `+=` before moving on.

## Testing a `Result`

Let's give the cart two more abilities. A discount takes a percentage off the
total, and a percentage outside 0 to 100 is a mistake by whoever configured the
discount, so it's a `Result`, following the reasoning in [Chapter
9](/book/ch09-03-to-trap-or-not). And prices need to be shown as dollars and
cents.

*Filename: src/cart.au*

```aura
public def subtotal(prices: list[int64]) -> int64:
    mut total = 0
    for price in prices:
        total += price
    return total

public def apply_discount(total: int64, percent: int64) -> Result[int64, str]:
    if percent < 0 or percent > 100:
        return Result.Err(f"discount must be between 0 and 100, got {percent}")
    return Result.Ok(total - total * percent // 100)

public def format_cents(cents: int64) -> str:
    return f"${cents // 100}.{cents % 100:02}"
```

<span class="caption">Listing 11-3: Adding `apply_discount` and `format_cents`</span>

There are two ways to test a function that returns a `Result`. When you know
exactly which value to expect, compare the whole `Result` with `==`, since
`Result` values compare structurally. When you only care about part of the
outcome, `match` on it and assert inside the arm. Here are tests that use both,
plus two for `format_cents`:

*Filename: tests/cart_test.au*

```aura fragment
from cart import subtotal, apply_discount, format_cents

def test_adds_prices():
    assert subtotal([250, 125]) == 375

def test_empty_cart_is_zero():
    assert subtotal([]) == 0

def test_ten_percent_off():
    assert apply_discount(2000, 10) == Result.Ok(1800)

def test_rejects_discount_over_100():
    match apply_discount(2000, 150):
        case Result.Ok(total):
            assert false, f"expected an error, got {total}"
        case Result.Err(message):
            assert message.contains("between 0 and 100")

def test_formats_cents():
    assert format_cents(1805) == "$18.05"

def test_formats_refunds():
    assert format_cents(-250) == "-$2.50"
```

<span class="caption">Listing 11-4: Tests for discounts and formatting</span>

`test_rejects_discount_over_100` uses `assert false` with a message for the
branch that shouldn't happen. An assertion message is evaluated only when the
assertion fails, so the f-string costs nothing on the passing path, and if the
branch is ever reached the message says what came back instead. In the `Err`
arm we check that the message mentions the valid range. We don't compare the
whole string, because the exact wording of an error message is the kind of
detail that changes without the behavior changing.

The last test is about refunds. A refund of $2.50 is stored as `-250` cents,
and it should print as `-$2.50`. Let's run everything:

```console
$ aura test
ok tests/cart_test.au::test_adds_prices
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/cart_test.au::test_ten_percent_off
ok tests/cart_test.au::test_rejects_discount_over_100
ok tests/cart_test.au::test_formats_cents
FAILED tests/cart_test.au::test_formats_refunds
error[AU4001]: assertion failed
 --> tests/cart_test.au:23:5
   |
23 |     assert format_cents(-250) == "-$2.50"
   |     ^
  = note: left = $-3.50
  = note: right = -$2.50
  = note: Aura call chain (innermost first): test_formats_refunds at 22:1
ok tests/smoke.au
6 passed; 1 failed
```

This time we didn't break anything on purpose. `format_cents` looked right, and
it is right for every positive amount, but `left = $-3.50` says a $2.50 refund
prints as minus three dollars fifty. The reason is in how Aura's `//` and `%`
treat negative numbers. `//` is floor division, so it rounds toward negative
infinity: `-250 // 100` is `-3`, not `-2`. And `%` takes the sign of the
divisor, so `-250 % 100` is `50`. Put together, they give `$-3.50`. Python
behaves the same way, which is why this bug turns up in so many languages.

The fix is to format the absolute value and put the sign in front:

*Filename: src/cart.au*

```aura
public def format_cents(cents: int64) -> str:
    if cents < 0:
        return "-" + format_cents(-cents)
    return f"${cents // 100}.{cents % 100:02}"
```

```console
$ aura test
ok tests/cart_test.au::test_adds_prices
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/cart_test.au::test_ten_percent_off
ok tests/cart_test.au::test_rejects_discount_over_100
ok tests/cart_test.au::test_formats_cents
ok tests/cart_test.au::test_formats_refunds
ok tests/smoke.au
7 passed; 0 failed
```

This is the everyday rhythm of testing: write down what the code should do, run
it, and let the failures tell you where your understanding and the code
disagree.

## What assertions can tell you

The `left` and `right` notes are what make a failing test quick to diagnose, so
it's worth knowing when you get them. Aura reports both operands when the whole
condition is a single comparison (`==`, `!=`, `<`, `<=`, `>`, or `>=`) or a
membership test with `in`. For `in`, the notes are labeled `item` and
`collection`. For anything else, such as two comparisons joined with `and`, a
`not in` test, or a function call that returns a `bool`, you get the plain
`assertion failed` message. The operands are still evaluated exactly once, left
to right, so capturing them never runs your code twice.

That suggests a habit. Prefer one comparison per assertion, so a failure shows
you the values. If you need to check two things, write two assertions. And when
a single comparison doesn't capture what you're checking, add a message that
includes the values you'd want to see.

Two other properties carry over from Chapter 9. The condition must be a `bool`,
so `assert items` is a compile error rather than a truthiness check. And
assertions are never removed, in tests or anywhere else, so a test can't pass
in one build mode and fail in another.

You've now seen the basic output of `aura test`. In the next section we'll look
at that output more closely, run a selection of tests instead of all of them,
and see what happens when the code under test traps.
