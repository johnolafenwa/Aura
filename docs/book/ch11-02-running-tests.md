# Running tests and reading failures

A test suite is only as useful as its output is readable. When something fails
at the end of a long day, you want to know which case failed, where, and with
what values, without scrolling. This section looks closely at what `aura test`
prints, what happens when the code under test traps instead of failing an
assertion, and how to run only the tests you care about right now.

## A trap in the code under test

Let's add one more function to the cart: the average price of an item, which a
"you might also like" panel could use.

*Filename: src/cart.au*

```aura fragment
public def average_cents(prices: list[int64]) -> int64:
    return subtotal(prices) // prices.len()
```

Add it to the end of `src/cart.au`, add `average_cents` to the import line at
the top of `tests/cart_test.au`, and add two tests at the end of that file:

*Filename: tests/cart_test.au*

```aura fragment
def test_average_price():
    assert average_cents([100, 200, 600]) == 300

def test_average_of_empty_cart():
    assert average_cents([]) == 0
```

```console
$ aura test
ok tests/cart_test.au::test_adds_prices
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/cart_test.au::test_ten_percent_off
ok tests/cart_test.au::test_rejects_discount_over_100
ok tests/cart_test.au::test_formats_cents
ok tests/cart_test.au::test_formats_refunds
ok tests/cart_test.au::test_average_price
FAILED tests/cart_test.au::test_average_of_empty_cart
error[AU4004]: division by zero
 --> tests/cart_test.au:18:12
   |
18 | 
   |            ^
  = note: Aura call chain (innermost first): cart::average_cents at 17:8 -> test_average_of_empty_cart at 28:1
ok tests/smoke.au
8 passed; 1 failed
```

The empty cart never reached the assertion. `prices.len()` is 0, so the
division trapped with `AU4004`, and as with any trap, the task running the test
stopped. The runner reports a trapped test exactly like a failed assertion:
`FAILED`, the case name, and the diagnostic. You don't need to do anything
special to find bugs that trap; every trap in a test is a failure.

The call chain is the line to read here. It says the trap happened in
`cart::average_cents`, which is declared at line 17 of the `cart` module,
called from `test_average_of_empty_cart`. Functions from other modules are
shown with their module name, so you can tell your test code from the code
under test at a glance.

You may have noticed something odd about the `-->` line, though. The trap
happened at line 18, column 12 of `src/cart.au`, which is the `//` in
`average_cents`. But the diagnostic names `tests/cart_test.au` and prints that
file's line 18, which happens to be blank. This is a known defect in Aura 0.11:
when a trap happens inside an imported module, the location shows the right
line and column but the wrong file. Until it's fixed, trust the call chain for
the function and use the line number in that function's file.

### Deciding what the right answer is

The trap is the test doing its job, but it raises a design question rather than
revealing a typo: what *is* the average price of an empty cart? There isn't
one. Returning 0 would be a convenient lie that might show "$0.00" to a
customer. An empty cart is an ordinary state, not a mistake, so following
Chapter 9's rules of thumb, the function should return `int64 | None`:

*Filename: src/cart.au*

```aura fragment
public def average_cents(prices: list[int64]) -> int64 | None:
    if prices.len() == 0:
        return None
    return subtotal(prices) // prices.len()
```

The test changes with it. `is None` checks for the absent case:

*Filename: tests/cart_test.au*

```aura fragment
def test_average_of_empty_cart():
    assert average_cents([]) is None
```

```console
$ aura test
ok tests/cart_test.au::test_adds_prices
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/cart_test.au::test_ten_percent_off
ok tests/cart_test.au::test_rejects_discount_over_100
ok tests/cart_test.au::test_formats_cents
ok tests/cart_test.au::test_formats_refunds
ok tests/cart_test.au::test_average_price
ok tests/cart_test.au::test_average_of_empty_cart
ok tests/smoke.au
9 passed; 0 failed
```

`test_average_price` didn't need to change. Its `== 300` compares an `int64 |
None` with an `int64`, which is true when the optional holds 300.

This is a pattern worth noticing. Writing the test for an edge case forced a
decision about the function's contract, and the decision showed up in its type,
where every caller will see it.

## Where the output goes

Back when `test_average_of_empty_cart` was failing, the output looked like one
stream, but it was two. The `ok` lines and the final summary go to standard
output. Each `FAILED` line and its diagnostic go to standard error. Hiding
standard error shows the split:

```console
$ aura test 2>/dev/null
ok tests/cart_test.au::test_adds_prices
ok tests/cart_test.au::test_empty_cart_is_zero
ok tests/cart_test.au::test_ten_percent_off
ok tests/cart_test.au::test_rejects_discount_over_100
ok tests/cart_test.au::test_formats_cents
ok tests/cart_test.au::test_formats_refunds
ok tests/cart_test.au::test_average_price
ok tests/smoke.au
8 passed; 1 failed
```

The summary still counts the failure, so a script reading only standard output
knows something went wrong. Scripts usually look at the exit status instead.
`aura test` exits with 0 when every selected case passes, 1 when any case
fails, and 2 when the command itself was used wrongly, such as an unknown
option. A CI job that runs `aura test` and stops on a non-zero status needs
nothing else.

Tests can print too. The runner captures each case's standard output and prints
it immediately before that case's result line, so it stays next to the case
that produced it. Here is a test that prints a progress message and then fails,
with a custom assertion message:

*Filename: tests/limit_test.au*

```aura fragment
from cart import subtotal

def test_big_cart_stays_under_limit():
    prices = [4999, 3500, 2750]
    total = subtotal(prices)
    print(f"checking {prices.len()} items")
    assert total <= 10000, f"cart total {total} is over the 10000 cent limit"
```

```console
$ aura test tests/limit_test.au
checking 3 items
FAILED tests/limit_test.au::test_big_cart_stays_under_limit
error[AU4001]: cart total 11249 is over the 10000 cent limit
 --> tests/limit_test.au:7:5
  |
7 |     assert total <= 10000, f"cart total {total} is over the 10000 cent limit"
  |     ^
  = note: left = 11249
  = note: right = 10000
  = note: Aura call chain (innermost first): test_big_cart_stays_under_limit at 3:1
0 passed; 1 failed
```

The `print` output appears first, then the failure. The custom message replaced
`assertion failed` as the diagnostic's headline, and because the condition is
still a single comparison, the `left` and `right` notes are there as well. A
message and operand values work together; you don't have to choose. We only
wrote this file to see the output, so delete `tests/limit_test.au` before going
on.

## Running a subset of tests

The command above passed a path: `aura test tests/limit_test.au` ran only that
file. You can pass any number of files or directories, and `aura test` runs the
`.au` files in them instead of the whole `tests/` tree:

```console
$ aura test tests/smoke.au
ok tests/smoke.au
1 passed; 0 failed
```

To select by name instead, use `-k` with a piece of the case name. The full
name, including the file path, is matched against the text you give:

```console
$ aura test -k average
ok tests/cart_test.au::test_average_price
FAILED tests/cart_test.au::test_average_of_empty_cart
error[AU4004]: division by zero
 --> tests/cart_test.au:18:12
   |
18 | 
   |            ^
  = note: Aura call chain (innermost first): cart::average_cents at 17:8 -> test_average_of_empty_cart at 28:1
1 passed; 1 failed
```

(This run is from before we fixed `average_cents`.) Only the two cases with
`average` in their names ran; `smoke.au` and the other cart tests were skipped
and aren't counted. When you're working on one function, this is the loop
you'll live in: change the code, run `aura test -k` with the function's name,
repeat, and run the full suite before you commit.

The match is a plain substring, and it's case-sensitive. A filter that matches
nothing isn't an error, which is worth knowing, because a typo looks like
success:

```console
$ aura test -k Average
0 passed; 0 failed
```

If you see `0 passed; 0 failed` when you expected tests to run, check the
spelling and case of the filter. Leaving the filter's value empty is a usage
error, which exits with status 2:

```console
$ aura test -k
aura test -k requires a non-empty substring
```

## Timeouts

A test that loops forever would hang the whole suite, and in CI that can mean
an hour of waiting for a job that was never going to finish. So every case has
a time limit, 30 seconds by default. Here's a deliberately stuck test, run with
a much shorter limit using `--timeout-ms`:

*Filename: tests/slow_test.au*

```aura
def test_waits_forever():
    mut ticks = 0
    while ticks >= 0:
        ticks = 0
```

```console
$ aura test --timeout-ms 200 tests/slow_test.au
FAILED tests/slow_test.au::test_waits_forever (timed out after 200ms)
0 passed; 1 failed
```

A timed-out case is a failure, and the reason in parentheses says why. There's
no diagnostic, because nothing trapped; the runner stopped waiting. Raise the
limit with `--timeout-ms` for slow integration tests that talk to real files or
processes, rather than splitting them up to fit. Delete `tests/slow_test.au`
now; we don't want a test that can never pass.

## Mistakes in a test's shape

The runner is strict about what counts as a test, and tells you when a function
looks like a test but isn't one. Suppose you try to give a test a parameter,
hoping to run it with several percentages:

*Filename: tests/discount_test.au*

```aura fragment
from cart import apply_discount

def test_discount(percent: int64):
    assert apply_discount(1000, percent) == Result.Ok(1000 - 10 * percent)
```

```console
$ aura test tests/discount_test.au
FAILED tests/discount_test.au::test_discount (test function `test_discount` must be parameterless)
0 passed; 1 failed
```

The runner has no way to know what to pass, so a `test_` function with
parameters is reported as a failure, not skipped. Delete
`tests/discount_test.au` for now. Running one test body with several inputs is
a real need, and Aura has a way to do it, which we'll see in the next section
along with the other tools for organizing a growing suite.
