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
truthiness. The optional message must have type `String`.

## Evaluation Is Deliberately Lazy

Aura evaluates the condition exactly once. When it is `true`, execution
continues and the message expression is not evaluated. When it is `false`, the
message is evaluated exactly once and becomes the failure text. This makes it
safe to construct an expensive diagnostic only for the failure path:

```python
def explain(value: int64) -> String:
    print("building failure message")
    return f"unexpected value {value}"

value = 4
assert value == 4, explain(value)
```

This program does not print `building failure message`.

If evaluating the condition or message fails first, that earlier failure is
reported instead of an assertion failure.

## Failure Behavior

`assert false` fails with diagnostic code `AU4001` and the exact message
`assertion failed`. A custom message is preserved exactly, including an empty
or whitespace-only String. The diagnostic points to the `assert` keyword.

Assertions are never removed by an optimization or release mode. Aura has no
assertion-stripping option, so do not use an assertion as a substitute for
recoverable validation of untrusted input. Return a typed `Result` for a
failure the caller should handle.

Active `with` cleanups still run when an assertion fails. If cleanup also
fails, the assertion remains the primary diagnostic.

## Assertions In Test Files

The existing file-level test runner treats an assertion failure like any other
test-program failure:

```bash
aura test tests/check_account.au
```

A file whose assertions all pass succeeds; a failing assertion is rendered
with its source span and makes the file fail. Function-level `test_*`
discovery is a separate later compiler milestone.

Run the maintained example with:

```bash
cargo run -p aura -- run examples/basics/assertions.au
```
