# Assertions

An assertion states an invariant that must hold for execution to continue.
Assertions are for programmer errors and internal consistency, not recoverable
input or protocol failures. Use a typed `Result` when a caller should be able
to handle the outcome.

## Grammar

The two forms are:

    assert condition
    assert condition, message

The normative production is
`assert-statement = "assert", non-tuple-expression, [ ",", non-tuple-expression ], statement-end`.
The top-level comma belongs to the assertion statement rather than either
operand. There is no parenthesized statement form, trailing comma, or
additional argument.

Either operand may use ordinary delimiter continuation. For example, the
condition may be grouped across physical lines; that remains an assertion
statement and does not create an `assert(...)` call form.

`assert` is a reserved keyword. An assertion is valid anywhere an ordinary
statement is valid, including a script-style entry module. A file still cannot
combine executable top-level statements with a local `main`.

## Typing Rules

The condition must have exactly type `bool`; numbers, strings, collections,
resources, and class values are not converted by truthiness. The optional
message must have exactly type `str`.

An assertion is a fallthrough statement for static control-flow analysis. It
does not establish a permanent narrowing or value refinement, and a statically
false condition is not treated as a substitute for a return.

## Runtime Semantics

The condition evaluates exactly once. If it is `true`, execution continues and
the optional message is not evaluated. If it is `false`, the message evaluates
exactly once and the assertion traps.

Without a message, the exact diagnostic text is `assertion failed`. A custom
message is preserved exactly, including an empty or whitespace-only str. A
trap while evaluating the condition or message occurs first and prevents the
assertion trap.

### Operand Introspection

When the whole condition is one of the following non-consuming operations, a
failed assertion reports the two values used by that operation:

    assert left == right
    assert left != right
    assert left < right
    assert left <= right
    assert left > right
    assert left >= right
    assert item in collection

Parentheses around the whole condition preserve introspection. Each operand is
evaluated once from left to right. The comparison or membership operation uses
those captured values. The values are rendered only on the failure edge with
the ordinary `str()` contract, before the lazy message is evaluated.

Comparison chains, `not in`, Boolean combinations, calls that return `bool`,
and consuming custom comparison dispatch keep the ordinary assertion
diagnostic without operand values. Aura does not clone, move twice, or make a
second observation of an operand to produce a diagnostic. Membership is the
builtin operation on `list`, `dict`, `set`, and `str`; Aura has no custom
membership protocol.

The following verified program demonstrates successful fallthrough and a lazy
message:

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

## Ownership And Evaluation Order

Condition effects complete before any message effect. Values moved, copied,
borrowed, or mutated while evaluating either expression obey the ordinary
expression rules; an assertion inserts no hidden clone. Because the message
belongs only to the false branch, its moves and mutations do not occur on the
true path.

Assertion failure exits active `with` scopes. Registered cleanup runs exactly
once in reverse nesting order. The assertion diagnostic is established before
cleanup begins and remains primary if cleanup also fails.

## Diagnostics

`AU2002` reports a condition whose type is not `bool` or a message whose type
is not `str`. The primary location points at the `assert` keyword.

`AU4001` reports a failed assertion at runtime. It uses the same keyword location
and the exact default or custom message described above. A condition or message
trap keeps its own diagnostic code, message, and span instead.

An introspected comparison appends `left = ...` and `right = ...` notes in
operand order. Membership appends `item = ...` and `collection = ...`. Each
rendered value is bounded to 4,096 UTF-8 bytes. Longer values end with
`... (truncated)` at a valid UTF-8 boundary.

Structured diagnostic schema 1 includes `assertion_operands` only for an
introspected failure. Its two entries contain `label`, `type`, `value`, and
`truncated`. Other diagnostics omit the field.

## Backend Support

The checker and MIR lowering are shared. `aura run`, directly emitted native
programs, and auto-backend builds preserve the same evaluation order, exact
messages, `AU4001` keyword span, standard-output ordering, and cleanup
precedence. File-level `aura test` reports an assertion trap as a failed test
program with the same diagnostic. Function-level `test_*` cases and JSON test
records preserve the same structured operand data.

## Limits And Implementation-Defined Behavior

Aura has no assertion-stripping mode, optimization flag, environment
switch, or backend option. Every accepted assertion executes in every build.
Message contents are preserved exactly. The human diagnostic renderer adds its
normal `error[AU4001]` prefix, source context, and bounded operand notes when
the condition qualifies for introspection.

Assertion failure terminates the current Aura execution path; it is not a
catchable exception. Use `Result` for recoverable validation.

## Status

Both assertion forms and their base sequencing, cleanup, top-level, and
no-strip behavior are accepted under ADR-0024. Aura 0.3 adds the bounded
two-operand diagnostic contract under ADR-0045. Exception statements, `raise`,
and catchable assertion failures are not part of Aura 0.3.
