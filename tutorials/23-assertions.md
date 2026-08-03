# Assertions

Assertions turn a condition that must hold into an immediate, source-located
runtime failure. Use the short form when the default message is enough:

```python
assert user_count >= 0
```

Use a second expression when the failure needs application context:

```python
assert response_code == 200, "worker expected a successful response"
```

The condition must have type `bool`. Aura does not apply Python-style
truthiness. The optional message must have type `str`.

## Evaluation Is Deliberately Lazy

Aura evaluates the condition exactly once. When it is `true`, execution
continues and the message expression is not evaluated. When it is `false`, the
message is evaluated exactly once and becomes the failure text. This makes it
safe to construct an expensive diagnostic only for the failure path:

```python
def explain(value: int64) -> str:
    print("building failure message")
    return f"unexpected value {value}"

value = 4
assert value == 4, explain(value)
```

This program does not print `building failure message`.

If evaluating the condition or message fails first, Aura reports that earlier
failure and never reaches the assertion result.

## Failed Comparisons Show Their Values

For a top-level comparison or positive membership test, Aura reports the two
values that produced the failure:

```python
expected = 42
actual = 41
assert actual == expected
```

The diagnostic includes:

```text
left = 41
right = 42
```

`assert item in collection` uses `item` and `collection` labels. Operands still
evaluate exactly once from left to right. A custom message remains lazy and is
evaluated after the failed operands have been captured. Each displayed value
is limited to 4,096 UTF-8 bytes and receives a visible truncation suffix when
needed.

This focused view applies to `==`, `!=`, `<`, `<=`, `>`, `>=`, and positive
`in` when the operation reads both operands without consuming them. Comparison
chains, `not in`, Boolean combinations, and calls returning `bool` retain the
ordinary assertion failure message.

## Failure Behavior

`assert false` fails with diagnostic code `AU4001` and the exact message
`assertion failed`. A custom message is preserved exactly, including an empty
or whitespace-only str. The diagnostic points to the `assert` keyword.

Assertions are never removed by an optimization or release mode. Aura has no
assertion-stripping option, so do not use an assertion as a substitute for
recoverable validation of untrusted input. Return a typed `Result` for a
failure the caller should handle.

Active `with` cleanups still run when an assertion fails. If cleanup also
fails, the assertion remains the primary diagnostic.

## Assertions In Test Files

`aura test` discovers parameterless module functions named `test_*`. A function
returning `None` is one independently reported case:

```python
def test_account_total():
    charges = [20, 21]
    assert charges[0] + charges[1] == 41
```

The canonical name is `path::test_account_total`. Functions are discovered in
source order. A file with no `test_*` function remains one file-level case and
runs through `main()` or top-level statements.

Use `-k` to select a literal, case-sensitive substring of the complete case
name:

```bash
aura test -k account tests/check_account.au
```

A valid filter with no matches succeeds with a zero-case summary.

### Per-Case Lifecycle

Optional `setup()` and `teardown()` functions run around every selected case:

```python
def setup():
    print("setup")

def teardown():
    print("teardown")

def test_total():
    print("case")
    assert 20 + 21 == 41
```

The observable order is `setup`, `case`, `teardown`. Teardown still runs when
setup or the case traps. The earlier failure remains primary, and a teardown
failure is reported secondarily. Each phase enters the same checked module in
isolation, so Aura values and module state do not flow between phases. External
effects such as file writes can be used when a test needs observable lifecycle
state.

### Parameterized Cases

A registration function returns labeled, named test functions:

```python
def empty_case():
    assert "".len() == 0

def unicode_case():
    assert "A🎉".len() == 2

def test_lengths() -> list[(str, def() -> None)]:
    return [("empty", empty_case), ("unicode", unicode_case)]
```

The two case names end in `test_lengths[empty]` and
`test_lengths[unicode]`. Registration happens once before `-k` filtering.
Labels must be non-empty and unique. Returned functions are capture-free,
parameterless, repeatable, and return `None`; captured closures are rejected.
Setup and teardown run for each selected expanded case, not for registration.

### JSON Results

`aura test --format json` emits one schema-version-1 document. Its summary
contains selected, passed, and failed counts. Each ordered test record contains
the canonical name, file, outcome, and lifecycle duration in milliseconds.
Captured stdout is included when non-empty. A trapped case carries the normal
structured diagnostic, including assertion operands; a runner failure carries
a reason. A teardown failure accompanying an earlier failure appears as a
secondary teardown record.

Human and JSON runs exit 0 when all selected cases pass and 1 when any case or
discovery step fails. Usage errors exit 2.

The maintained example works both as an ordinary program and as a test module:

```bash
cargo run -p aura -- run examples/basics/assertions.au
cargo run -p aura -- test examples/basics/assertions.au
cargo run -p aura -- test --format json -k '[unicode]' examples/basics/assertions.au
```
