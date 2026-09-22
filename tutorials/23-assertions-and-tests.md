# Assertions And Tests

An assertion states a condition that must hold. If the condition is false,
the program fails at once and the failure points to the source line. Use the
short form when the default message is enough:

```aura fragment
assert user_count >= 0
```

Add a second expression when the failure needs context:

```aura fragment
assert response_code == 200, "worker expected a successful response"
```

The condition must have type `bool`. Aura does not apply Python-style
truthiness. The optional message must have type `str`.

## Evaluation Is Deliberately Lazy

Aura evaluates the condition exactly once. When it is `true`, execution
continues and the message is never evaluated. When it is `false`, Aura
evaluates the message exactly once and uses it as the failure text. So you can
build an expensive message that only runs on failure:

```aura check-pass
def explain(value: int64) -> str:
    print("building failure message")
    return f"unexpected value {value}"

value = 4
assert value == 4, explain(value)
```

This program does not print `building failure message`.

If evaluating the condition or the message fails first, Aura reports that
earlier failure. The assertion result is never reached.

## Failed Comparisons Show Their Values

When a top-level comparison or a positive membership test fails, Aura reports
both values:

```aura check-pass
expected = 42
actual = 41
assert actual == expected
```

The diagnostic includes:

```text
left = 41
right = 42
```

For `assert item in collection`, the labels are `item` and `collection`.

The operands still evaluate exactly once, from left to right. A custom message
stays lazy and is evaluated after Aura captures the failed operands. Each
displayed value is limited to 4,096 UTF-8 bytes. A longer value gets a
visible truncation suffix.

This view covers `==`, `!=`, `<`, `<=`, `>`, `>=`, and positive `in`, when
the operation reads both operands without consuming them. Other forms keep the
ordinary assertion failure message:

- comparison chains
- `not in`
- Boolean combinations
- calls that return `bool`

## Failure Behavior

`assert false` fails with diagnostic code `AU4001` and the exact message
`assertion failed`. A custom message is kept exactly, even when it is empty or
only whitespace. The diagnostic points to the `assert` keyword.

Aura never removes assertions, in any optimization or release mode, and has
no option to strip them. So do not use an assertion to validate untrusted
input. For a failure the caller should handle, return a typed `Result`.

Active `with` cleanups still run when an assertion fails. If a cleanup also
fails, the assertion stays the primary diagnostic.

## Assertions In Test Files

`aura test` finds parameterless module functions whose names start with
`test_`. Each such function that returns `None` is one test case, reported on
its own:

```aura check-pass
def test_account_total():
    charges = [20, 21]
    assert charges[0] + charges[1] == 41
```

The case's canonical name is `path::test_account_total`. Aura discovers
functions in source order. A file with no `test_*` function is one file-level
case, which runs through `main()` or the top-level statements.

Use `-k` to select cases whose full name contains a literal, case-sensitive
substring:

```bash
aura test -k account tests/check_account.au
```

A valid filter that matches nothing succeeds with a zero-case summary.

### Per-Case Lifecycle

Optional `setup()` and `teardown()` functions run around every selected case:

```aura check-pass
def setup():
    print("setup")

def teardown():
    print("teardown")

def test_total():
    print("case")
    assert 20 + 21 == 41
```

This prints `setup`, `case`, `teardown`, in that order.

Teardown still runs when setup or the case traps. The earlier failure stays
primary, and a teardown failure is reported as secondary.

Setup, the case, and teardown each enter the same checked module in isolation.
Aura values and module state do not flow from one step to the next. When a
test needs lifecycle state it can observe, use an external effect such as a
file write.

### Parameterized Cases

A registration function returns a list of labels paired with named test
functions:

```aura check-pass
def empty_case():
    assert "".len() == 0

def unicode_case():
    assert "A🎉".len() == 2

def test_lengths() -> list[(str, def() -> None)]:
    return [("empty", empty_case), ("unicode", unicode_case)]
```

The two case names end in `test_lengths[empty]` and `test_lengths[unicode]`.

The rules for registration:

- Registration runs once, before `-k` filtering.
- Labels must be non-empty and unique.
- Returned functions must be capture-free, parameterless, and repeatable, and
  must return `None`. A captured closure is rejected.
- Setup and teardown run for each selected expanded case. They do not run for
  registration.

### JSON Results

`aura test --format json` emits one schema-version-1 document:

- The summary has selected, passed, and failed counts.
- Each test record, in order, has the canonical name, file, outcome, and
  lifecycle duration in milliseconds.
- A record includes captured stdout when it is non-empty.
- A trapped case carries the normal structured diagnostic, including assertion
  operands. A runner failure carries a reason.
- A teardown failure that follows an earlier failure appears as a secondary
  teardown record.

Human and JSON runs share the same exit codes:

| Exit code | Meaning |
| --- | --- |
| 0 | Every selected case passed. |
| 1 | A case or the discovery step failed. |
| 2 | Usage error. |

The maintained example works both as an ordinary program and as a test module:

```bash
cargo run -p aura -- run examples/basics/assertions.au
cargo run -p aura -- test examples/basics/assertions.au
cargo run -p aura -- test --format json -k '[unicode]' examples/basics/assertions.au
```
