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

`aura test` discovers parameterless `test_*` functions and reports each one as
an independent case. It treats an assertion failure like any other test
failure:

```bash
aura test tests/check_account.au
```

A file whose assertions all pass succeeds; a failing assertion is rendered
with its source span and makes the case fail. `aura test --format json`
preserves introspected operands as structured fields, and `-k substring`
selects matching canonical case names.

Run the maintained example with:

```bash
cargo run -p aura -- run examples/basics/assertions.au
```
