# Assertions

An assertion states an invariant that must hold for execution to continue. Use assertions for programmer errors and internal consistency. Do not use them for recoverable input or protocol failures. When a caller should be able to handle the outcome, return a typed `Result`.

## Grammar

The two forms are:

    assert condition
    assert condition, message

The normative production is
`assert-statement = "assert", non-tuple-expression, [ ",", non-tuple-expression ], statement-end`.

The top-level comma belongs to the assertion statement, not to either operand. There is no parenthesized statement form, no trailing comma, and no additional argument.

Either operand may use ordinary delimiter continuation. For example, the condition may be grouped across physical lines. The result is still an assertion statement, not an `assert(...)` call form.

`assert` is a reserved keyword. An assertion is valid anywhere an ordinary statement is valid, including a script-style entry module. A file still cannot combine executable top-level statements with a local `main`.

## Typing Rules

The condition must have exactly type `bool`. Numbers, strings, collections, resources, and class values are not converted by truthiness. The optional message must have exactly type `str`.

Static control-flow analysis treats an assertion as a fallthrough statement. An assertion does not establish a permanent narrowing or value refinement. A statically false condition does not substitute for a return.

## Runtime Semantics

The condition evaluates exactly once.

- If it is `true`, execution continues and the message is not evaluated.
- If it is `false`, the message evaluates exactly once and the assertion traps.

Without a message, the exact diagnostic text is `assertion failed`. A custom message is kept exactly, including an empty or whitespace-only `str`. A trap while evaluating the condition or the message occurs first and prevents the assertion trap.

This verified program shows successful fallthrough and a lazy message:

```aura
def build_message() -> str:
    print("message evaluated")
    return "unexpected arithmetic result"

def main():
    print("before")
    assert 2 + 2 == 4, build_message()
    assert true
    print("after")
```

Its output is:

```text
before
after
```

### Operand Introspection

When the whole condition is one of these non-consuming operations, a failed assertion reports the two values the operation used:

    assert left == right
    assert left != right
    assert left < right
    assert left <= right
    assert left > right
    assert left >= right
    assert item in collection

Parentheses around the whole condition keep introspection. Each operand is evaluated once, from left to right, and the comparison or membership operation uses those captured values. The values are rendered only on the failure edge, with the ordinary `str()` contract, before the lazy message is evaluated.

These conditions keep the ordinary assertion diagnostic, without operand values:

- comparison chains
- `not in`
- Boolean combinations
- calls that return `bool`
- consuming custom comparison dispatch

Aura does not clone, move twice, or observe an operand a second time to produce a diagnostic. Membership is the builtin operation on `list`, `dict`, `set`, and `str`. Aura has no custom membership protocol.

## Ownership And Evaluation Order

Condition effects complete before any message effect. Values moved, copied, borrowed, or mutated while evaluating either expression follow the ordinary expression rules. An assertion inserts no hidden clone. The message belongs only to the false branch, so its moves and mutations do not happen on the true path.

Assertion failure exits active `with` scopes. Registered cleanup runs exactly once, in reverse nesting order. The assertion diagnostic is established before cleanup begins, and it stays primary if cleanup also fails.

## Diagnostics

| Code | Cause | Location and message |
| --- | --- | --- |
| `AU2002` | The condition's type is not `bool`, or the message's type is not `str`. | The primary location points at the `assert` keyword. |
| `AU4001` | An assertion failed at runtime. | The same keyword location, with the exact default or custom message described above. |

To fix `AU2002`, write an explicit comparison such as `count != 0` instead of relying on truthiness, and pass a `str` message.

A trap in the condition or message keeps its own diagnostic code, message, and span instead of `AU4001`.

An introspected failure appends notes in operand order:

- a comparison appends `left = ...` and `right = ...`
- membership appends `item = ...` and `collection = ...`

Each rendered value is bounded to 4,096 UTF-8 bytes. A longer value ends with `... (truncated)` at a valid UTF-8 boundary.

Structured diagnostic schema 1 includes `assertion_operands` only for an introspected failure. Its two entries contain `label`, `type`, `value`, and `truncated`. Other diagnostics omit the field.

## Backend Support

The checker and MIR lowering are shared. `aura run`, directly emitted native programs, and auto-backend builds preserve the same:

- evaluation order
- exact messages
- `AU4001` keyword span
- standard-output ordering
- cleanup precedence

File-level `aura test` reports an assertion trap as a failed test program with the same diagnostic. Function-level `test_*` cases and JSON test records preserve the same structured operand data.

## Limits And Implementation-Defined Behavior

Aura has no assertion-stripping mode, optimization flag, environment switch, or backend option. Every accepted assertion executes in every build.

Message contents are kept exactly. The human diagnostic renderer adds its normal `error[AU4001]` prefix and source context. When the condition qualifies for introspection, it also adds the bounded operand notes.

Assertion failure ends the current Aura execution path. It is not a catchable exception. Use `Result` for recoverable validation.

## Status

Both assertion forms are accepted, with the sequencing, cleanup, top-level, and no-strip behavior on this page. Aura 0.3 includes the bounded two-operand diagnostic.

Aura 0.3 has no exception statements, no `raise`, and no catchable assertion failures.

Design record: `architecture_docs/decisions/0024-assertion-evaluation-and-diagnostic-policy.md` for the assertion forms and their behavior, and `architecture_docs/decisions/0045-testing-framework-and-assertion-introspection.md` for operand introspection.
