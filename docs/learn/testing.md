# Testing

The compiler checks ownership and failure, but it cannot tell you whether
your logic is right. Tests do that. Aura ships its own test runner, so you do
not need to pick one.

## Your First Test

Tests live in `tests/`, next to your package manifest. A test is a function
with no parameters whose name starts with `test_`:

```aura
def subtotal(prices: list[float64]) -> float64:
    mut sum = 0.0
    for price in prices:
        sum += price
    return sum

def test_adds_prices():
    assert subtotal([1.5, 2.5]) == 4.0

def test_empty_list_is_zero():
    assert subtotal([]) == 0.0
```

Run every test in the package:

```bash
aura test
```

```text
ok tests/subtotal_test.au::test_adds_prices
ok tests/subtotal_test.au::test_empty_list_is_zero
2 passed; 0 failed
```

Each case is named `path::function`, so a failure tells you which file and
which function to open. The command exits non-zero when any case fails,
which is all a CI job needs.

## Reading A Failure

Change the expected total to a wrong value and run the tests again:

```text
FAILED tests/subtotal_test.au::test_wrong_expectation
error[AU4001]: assertion failed
 --> tests/failing_test.au:8:5
  |
8 |     assert subtotal([1.5, 2.5]) == 5.0
  |     ^
  = note: left = 4.0
  = note: right = 5.0
  = note: Aura call chain (innermost first): test_wrong_expectation at 7:1
```

When a comparison in `assert` fails, Aura reports both sides: `left = 4.0`
and `right = 5.0`. The compiler arranges this ahead of time. You do not need
to add print statements and rerun to see the values.

An assertion can take a message. Aura evaluates it only when the assertion
fails:

```aura
def test_port_is_in_range():
    port = 8080
    assert port > 1024, f"port {port} is reserved"
```

## One Test, Many Cases

To run the same logic against several inputs, return a list of labeled case
functions from a `test_*` function:

```aura
def parse_port(text: str) -> int64 | None:
    match parse_int64(text):
        case Result.Ok(port):
            return port
        case Result.Err(_):
            return None

def valid_case():
    assert parse_port("8080") == 8080

def empty_case():
    assert parse_port("") == None

def test_ports() -> list[(str, def() -> None)]:
    return [("valid", valid_case), ("empty", empty_case)]
```

Each entry becomes its own case, reported and counted separately:

```text
ok tests/parse_test.au::test_ports[valid]
ok tests/parse_test.au::test_ports[empty]
2 passed; 0 failed
```

The case functions are ordinary function values, the same first-class
functions you can pass anywhere else. They must take no arguments and
capture nothing.

## Setup And Teardown

A file may define `setup()` and `teardown()`. They run around every case in
that file:

```aura
def setup():
    print("setup")

def teardown():
    print("teardown")

def test_total():
    print("case")
    assert 20 + 21 == 41
```

The order is `setup`, then the case, then `teardown`. Teardown runs even when
the case fails, so a temporary file or spawned process is cleaned up either
way.

Each phase runs in isolation, so module values do not carry state from one
phase to the next. When a test needs to observe lifecycle state, use the
filesystem or another external effect.

## Running A Subset

While you work on one thing, run only that thing. `-k` matches a substring of
the full case name:

```bash
aura test -k valid
```

```text
ok tests/parse_test.au::test_ports[valid]
1 passed; 0 failed
```

You can also pass files or directories instead of the default `tests/` tree:

```bash
aura test tests/parse_test.au
```

## Tests In CI

`--format json` prints one machine-readable document instead of progress
lines. It contains a `schema_version`, a summary, and one record per case
with its duration and any failure diagnostic:

```bash
aura test --format json
```

Each case has a 30-second timeout by default. Use `--timeout-ms` to change it
for slow integration tests.

## Where To Go Next

A file in `tests/` with no `test_*` function still runs as a single case,
through `main()` or its top-level statements. Use this for end-to-end scripts
that you want executed rather than asserted.

The [Assertions](/manual/assertions) chapter is the normative reference for
`assert`, operand reporting, and evaluation order.
[CLI And Tooling](/manual/cli-and-tooling) specifies test discovery,
selection, the JSON schema, and exit codes.
