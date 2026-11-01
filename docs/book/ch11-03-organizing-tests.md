# Organizing tests

A handful of tests fits comfortably in one file. A real package collects
hundreds, some fast and pure, some that touch files or start processes, some
that run the same check over a table of inputs. Keeping that collection
readable takes four things from the runner: a way to split tests across files,
a way to generate cases from a list, output a CI system can read, and setup
that runs around each case.

## Files, helpers, and what tests can see

`aura test` reads every `.au` file under `tests/`, including subdirectories, so
you can group tests however suits the package. A common choice is one test file
per module or per feature, named after what it tests: `tests/cart_test.au` for
the cart, `tests/format_test.au` for formatting. Within a file, only
module-level functions whose names start with `test_` are tests. Every other
function is an ordinary helper that tests can call, which is how you share a
fixture builder or a custom check between several cases. Methods of classes are
never treated as tests, even if they're named `test_something`.

Test files see the package the way any other module does: through `import`, and
only the names marked `public`. Suppose we want to test a parsing helper in
`cart` directly, but haven't made it public:

*Filename: tests/parse_test.au*

```aura fragment
from cart import parse_prices

def test_skips_blank_lines():
    assert parse_prices("250\n\n125\n") == Result.Ok([250, 125])
```

```console
$ aura test tests/parse_test.au
FAILED tests/parse_test.au
error[AU1101]: item `parse_prices` is private in module `cart`
 --> tests/parse_test.au:1:18
  |
1 | from cart import parse_prices
  |                  ^
0 passed; 1 failed
```

A test file that doesn't compile fails as a whole, under its file name, before
any of its cases run. Aura has no special access for tests: if a test needs a
function, that function is part of the module's interface and should be
`public`. This is a deliberate limit. Tests that go through the public
interface keep working when you rearrange private code, and if something is
hard to test from outside, that's often a sign it should be its own small
public function. We'll make `parse_prices` public in a moment, when we add it
to `cart` properly.

## Parameterized cases

The formatting function has several cases worth checking: whole dollars,
single-digit cents, amounts under a dollar, refunds, and large amounts. Writing
a `test_` function for each works, but each test body is a single assertion,
and a failure in one would be easier to scan if they were grouped under one
name.

We saw in the last section that a test can't take parameters. Instead, a
`test_` function can *return* its cases. If it's declared to return `list[(str,
def() -> None)]`, a list of pairs of a label and a function, the runner calls
it once to collect the list and then runs each function as its own case:

*Filename: tests/format_test.au*

```aura fragment
from cart import format_cents

def whole_dollars():
    assert format_cents(1200) == "$12.00"

def single_digit_cents():
    assert format_cents(1205) == "$12.05"

def under_a_dollar():
    assert format_cents(99) == "$0.99"

def refund():
    assert format_cents(-250) == "-$2.50"

def large_amount():
    assert format_cents(123456789) == "$1,234,567.89"

def test_format_cents() -> list[(str, def() -> None)]:
    return [
        ("whole", whole_dollars),
        ("single-digit-cents", single_digit_cents),
        ("under-a-dollar", under_a_dollar),
        ("refund", refund),
        ("large", large_amount),
    ]
```

<span class="caption">Listing 11-5: One registration function that produces five cases</span>

```console
$ aura test tests/format_test.au
ok tests/format_test.au::test_format_cents[whole]
ok tests/format_test.au::test_format_cents[single-digit-cents]
ok tests/format_test.au::test_format_cents[under-a-dollar]
ok tests/format_test.au::test_format_cents[refund]
FAILED tests/format_test.au::test_format_cents[large]
error[AU4001]: assertion failed
 --> tests/format_test.au:16:5
   |
16 |     assert format_cents(123456789) == "$1,234,567.89"
   |     ^
  = note: left = $1234567.89
  = note: right = $1,234,567.89
  = note: Aura call chain (innermost first): large_amount at 15:1
4 passed; 1 failed
```

Each case is named after the registration function with its label in brackets,
`test_format_cents[large]`, and counted separately. The case functions are
ordinary named functions, the first-class function values you'll meet properly
in [Chapter 13](/book/ch13-00-closures-and-decorators). The type `def() ->
None` means "a function with no parameters that returns nothing". It
deliberately rules out closures that capture values, so every case can be run
on its own, in any order, without depending on the registration function's
local state. Labels must be non-empty and unique within the list.

The label is part of the case name, so `-k` can select a single case. Brackets
are ordinary characters to `-k`, which makes `[large]` a convenient filter:

```console
$ aura test -k "[large]"
FAILED tests/format_test.au::test_format_cents[large]
error[AU4001]: assertion failed
 --> tests/format_test.au:16:5
   |
16 |     assert format_cents(123456789) == "$1,234,567.89"
   |     ^
  = note: left = $1234567.89
  = note: right = $1,234,567.89
  = note: Aura call chain (innermost first): large_amount at 15:1
0 passed; 1 failed
```

The failure is a missing feature rather than a bug: we never wrote thousands
separators. Before we add them, let's look at the same failure the way a
machine would.

## Output for CI

The human output is designed for people. A CI system that wants to annotate a
pull request with the failing line, or a dashboard that tracks how long tests
take, needs something more regular. `--format json` replaces the progress lines
with a single JSON document on standard output. It's printed on one line; here
it is piped through a pretty-printer:

```console
$ aura test --format json -k "[large]" | python3 -m json.tool
{
    "schema_version": 1,
    "summary": {
        "failed": 1,
        "passed": 0,
        "selected": 1
    },
    "tests": [
        {
            "diagnostic": {
                "assertion_operands": [
                    {
                        "label": "left",
                        "truncated": false,
                        "type": "str",
                        "value": "$1234567.89"
                    },
                    {
                        "label": "right",
                        "truncated": false,
                        "type": "str",
                        "value": "$1,234,567.89"
                    }
                ],
                "call_frames": [
                    {
                        "function": "large_amount",
                        "span": {
                            "end": {
                                "column": 2,
                                "line": 15
                            },
                            "path": "/path/to/checkout/tests/format_test.au",
                            "start": {
                                "column": 1,
                                "line": 15
                            }
                        }
                    }
                ],
                "code": "AU4001",
                "edits": [],
                "help": [],
                "message": "assertion failed",
                "notes": [],
                "primary_span": {
                    "end": {
                        "column": 6,
                        "line": 16
                    },
                    "path": "tests/format_test.au",
                    "start": {
                        "column": 5,
                        "line": 16
                    }
                },
                "secondary_spans": [],
                "severity": "error",
                "task_ancestry": []
            },
            "duration_ms": 15,
            "file": "tests/format_test.au",
            "name": "tests/format_test.au::test_format_cents[large]",
            "outcome": "failed"
        }
    ]
}
```

Everything the human output showed is here as data. The `summary` has the
counts. Each entry in `tests` has the case's `name`, `file`, `outcome`, and
`duration_ms`; a failed case adds a `diagnostic` with the code, message, and
source span, and an assertion failure includes its two operands under
`assertion_operands`, each with its type and value. A case that wrote to
standard output also gets a `stdout` field. The `schema_version` lets tools
detect a future change of format instead of misreading it.

The exit status is the same in both formats: 0 when everything passed, 1 when
anything failed, 2 for a usage error. A CI step can therefore run `aura test`
(or `aura test --format json > results.json` to keep a report) and let the exit
status decide whether the job passes.

Now let's fix the formatting. Aura's format specifications include a `,` option
that groups the digits of an integer when it's paired with the `d` type, so the
whole-dollar part becomes `{cents // 100:,d}`:

*Filename: src/cart.au*

```aura
public def format_cents(cents: int64) -> str:
    if cents < 0:
        return "-" + format_cents(-cents)
    return f"${cents // 100:,d}.{cents % 100:02}"
```

All five formatting cases pass after this change.

## Setup and teardown

Some tests need the world to be in a particular state. Let's add a function
that loads prices from a file, one price per line. We'll split it in two:
`parse_prices` turns text into a list of prices, and `load_prices` reads a file
and hands its text to `parse_prices`. Here is the finished `cart` module, with
`import fs` added at the top for the file-reading function. We'll only use
`fs.read_to_string` here, and a couple of similar calls in the test below;
[Chapter 16](/book/ch16-01-files) covers the `fs` module properly.

*Filename: src/cart.au*

```aura
import fs

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
    if cents < 0:
        return "-" + format_cents(-cents)
    return f"${cents // 100:,d}.{cents % 100:02}"

public def average_cents(prices: list[int64]) -> int64 | None:
    if prices.len() == 0:
        return None
    return subtotal(prices) // prices.len()

public def parse_prices(text: str) -> Result[list[int64], str]:
    mut prices: list[int64] = []
    for line in text.split("\n"):
        if line.len() > 0:
            prices.append(try parse_int64(line))
    return Result.Ok(prices)

public def load_prices(path: str) -> Result[list[int64], str]:
    match own fs.read_to_string(path):
        case Result.Ok(text):
            return parse_prices(text)
        case Result.Err(error):
            return Result.Err(f"cannot read {path}: {error}")
```

<span class="caption">Listing 11-6: The finished `cart` module</span>

Splitting the function this way is a testing decision as much as a design one.
`parse_prices` is pure, so `tests/parse_test.au` from the start of this section
now passes, and we can test blank lines, bad numbers, and other parsing details
without touching the disk. Only `load_prices` needs a real file, and it needs
very few tests.

For those few, a test file can declare `setup()` and `teardown()` functions.
The runner calls `setup` before each case in the file and `teardown` after it:

*Filename: tests/load_test.au*

```aura fragment
import fs
from cart import load_prices, subtotal

FIXTURE = "prices_fixture.txt"

def setup():
    match fs.write_string(FIXTURE, "250\n125\n"):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            assert false, f"could not write fixture: {error}"

def teardown():
    match fs.remove_file(FIXTURE):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            assert false, f"could not remove fixture: {error}"

def test_loads_prices_from_file():
    match load_prices(FIXTURE):
        case Result.Ok(prices):
            assert subtotal(prices) == 375
        case Result.Err(message):
            assert false, message

def test_missing_file_is_an_error():
    match load_prices("no_such_file.txt"):
        case Result.Ok(_):
            assert false, "expected an error for a missing file"
        case Result.Err(message):
            print(message)
            assert message.starts_with("cannot read no_such_file.txt")
```

<span class="caption">Listing 11-7: Writing a fixture file before each case and removing it after</span>

Here is the whole suite, with every file in place:

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
ok tests/format_test.au::test_format_cents[whole]
ok tests/format_test.au::test_format_cents[single-digit-cents]
ok tests/format_test.au::test_format_cents[under-a-dollar]
ok tests/format_test.au::test_format_cents[refund]
ok tests/format_test.au::test_format_cents[large]
ok tests/load_test.au::test_loads_prices_from_file
cannot read no_such_file.txt: io.Error.NotFound
ok tests/load_test.au::test_missing_file_is_an_error
ok tests/parse_test.au::test_skips_blank_lines
ok tests/smoke.au
17 passed; 0 failed
```

The setup writes the file, the case reads it, and the teardown removes it, so
no fixture is left behind for the next case or the next run. The printed
message from `test_missing_file_is_an_error` appears right before its result,
as the previous section described.

Three properties of setup and teardown are worth knowing before you rely on them.

First, they run around every case, not once per file. A file with ten tests and
a `setup` runs `setup` ten times. That's slower than sharing one fixture, but
it means no case can leave behind state that changes the result of the next
one.

Second, setup can't hand values to the test. Each of the three phases (setup,
case, and teardown) starts fresh, and Aura values don't carry from one to the
next. Effects outside the program do, which is why our setup writes a file
instead of building a list in memory. If a case needs data that's cheap to
build, call a helper function from the test itself instead of using `setup`.

Third, teardown runs even when the case fails, which is what makes it safe for
cleanup. If teardown also fails, you see both failures, with the original one
first. Here's a small file where both go wrong:

*Filename: tests/teardown_test.au*

```aura
def teardown():
    assert false, "could not remove the fixture"

def test_total():
    assert 20 + 21 == 42
```

```console
$ aura test tests/teardown_test.au
FAILED tests/teardown_test.au::test_total
error[AU4001]: assertion failed
 --> tests/teardown_test.au:5:5
  |
5 |     assert 20 + 21 == 42
  |     ^
  = note: left = 41
  = note: right = 42
  = note: Aura call chain (innermost first): test_total at 4:1
teardown also failed for tests/teardown_test.au::test_total
error[AU4001]: could not remove the fixture
 --> tests/teardown_test.au:2:5
  |
2 |     assert false, "could not remove the fixture"
  |     ^
  = note: Aura call chain (innermost first): teardown at 1:1
0 passed; 1 failed
```

Delete `tests/teardown_test.au` once you've seen this. The one exception is a
timeout. A case that's still running when its time limit expires can't be
stopped safely, so its teardown isn't guaranteed to run. A test that times out
may leave its fixture behind, which is one more reason to keep fixtures in
files with fixed names that the next setup overwrites.

## Summary

You can now test Aura code with nothing but the `aura` command. A test is a
parameterless `test_` function that asserts what should be true, and the runner
reports each case by file and name, with the operands of a failed comparison
printed for you. A trap in the code under test fails the case with its full
diagnostic and call chain. You can narrow a run to a file, a directory, or any
case whose name contains a substring, set time limits, share setup and teardown
between cases, generate cases from a list, and produce JSON for CI, while the
exit status tells any script whether the suite passed.

Along the way the tests did more than catch mistakes. They found a real bug in
`format_cents`, forced a decision about the average of an empty cart, and
pushed `load_prices` apart into a pure parser and a thin file reader. That's
the usual effect of writing tests early, and it's the approach we'll take in
the next chapter. In [Chapter 12](/book/ch12-00-log-analyzer) we'll put
everything from the last few chapters together and build a command-line log
analyzer, starting from an empty directory.
