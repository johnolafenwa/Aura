# Testing

A language that checks ownership and failure at compile time still cannot
tell you whether your logic is right. That is what tests are for, and Aura
ships a runner so you do not have to pick one.

## Your First Test

Tests live in `tests/` next to your package manifest. A test is a
parameterless function whose name starts with `test_`:

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

Each case is named `path::function`, so a failure tells you exactly which
file and which function to open. The command exits non-zero when anything
fails, which is all a CI job needs.

## Reading A Failure

Change the expected total to something wrong and run it again:

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

`assert` is not a plain boolean check. When a comparison fails, the compiler
has already arranged for both sides to be reported: `left = 4.0`,
`right = 5.0`. You do not have to rerun anything with print statements to
find out what the values were.

Assertions take an optional message, evaluated only when the assertion
fails:

```aura
def test_port_is_in_range():
    port = 8080
    assert port > 1024, f"port {port} is reserved"
```

## One Test, Many Cases

When the same logic needs several inputs, return a list of labeled case
functions from a `test_*` function:

```aura
def parse_port(text: str) -> Option[int64]:
    match parse_int64(text):
        case Result.Ok(port):
            return Option.Some(port)
        case Result.Err(_):
            return Option.None

def valid_case():
    assert parse_port("8080") == Option.Some(8080)

def empty_case():
    assert parse_port("") == Option.None

def test_ports() -> list[(str, def() -> None)]:
    return [("valid", valid_case), ("empty", empty_case)]
```

Each entry becomes its own case, reported and counted separately:

```text
ok tests/parse_test.au::test_ports[valid]
ok tests/parse_test.au::test_ports[empty]
2 passed; 0 failed
```

The case functions are ordinary function values — the same first-class
functions you can pass anywhere else in the language. They must be
capture-free and take no arguments.

## Setup And Teardown

A file may define `setup()` and `teardown()`, which run around every case in
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
the case fails, so a temporary file or spawned process gets cleaned up either
way. Each phase runs in isolation, so state does not leak between them
through module values — use the filesystem or another external effect when a
test genuinely needs to observe lifecycle state.

## Running A Subset

While you are working on one thing, run only that thing. `-k` matches a
substring of the full case name:

```bash
aura test -k valid
```

```text
ok tests/parse_test.au::test_ports[valid]
1 passed; 0 failed
```

You can also pass explicit files or directories instead of the default
`tests/` tree:

```bash
aura test tests/parse_test.au
```

## Tests In CI

`--format json` prints one machine-readable document instead of progress
lines, with a `schema_version`, a summary, and one record per case including
its duration and failure diagnostic:

```bash
aura test --format json
```

Cases run under a 30-second timeout by default; `--timeout-ms` changes it for
slow integration tests.

## Where To Go Next

Any file in `tests/` without a `test_*` function still runs as a single case
through `main()` or its top-level statements, which is handy for end-to-end
scripts you want executed rather than asserted.

The [Assertions](/manual/assertions) chapter is the normative reference for
`assert`, operand reporting, and evaluation order, and
[CLI And Tooling](/manual/cli-and-tooling) specifies discovery, selection,
the JSON schema, and exit codes.
