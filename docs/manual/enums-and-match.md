# Enums And Pattern Matching

Enums define nominal sum types. Each value contains exactly one declared variant and, when that variant has payloads, one value for each payload position. Pattern matching evaluates a scrutinee once, selects the first matching arm, and binds payload values for that arm.

Aura uses enums for user data and for maintained runtime outcomes including `Option`, `Result`, queue operations, task waits, process status, supervisor events, and I/O errors.

## Enum Declarations

```python
enum Status:
    Ready(count: int32)
    Failed(str)
    Empty
```

A variant has one of three shapes:

- no payload, written without parentheses: `Empty`
- positional payloads, written as types: `Failed(str)` or `Pair(int32, int32)`
- named payloads, written as `name: Type`: `Ready(count: int32)`

One variant cannot mix positional and named payload declarations. Empty parentheses are not a payload-free declaration; omit them. Variant names must be unique within the enum. Every payload type must exist with the correct arity.

Enums may be generic and bounded:

```python
enum Load[T: Named]:
    Ready(T)
    Failed(message: str)
    Empty
```

Type parameters must be unique, substitutions are invariant, and every bound must be satisfied. See [Generics And Traits](/manual/generics-and-traits).

An enum is private to its defining module unless declared `public enum`. Individual variants do not carry separate visibility modifiers; importing a public enum exposes its variant constructor surface. Imported payload types must themselves be usable in the importing context for construction and matching to type-check.

The complete declaration and pattern syntax is in [Grammar](/manual/grammar#enums) and [Grammar](/manual/grammar#patterns-and-statement-matches).

## Construction

Use the enum type and variant name:

```python
ready = Status.Ready(count=3)
failed = Status.Failed("disk full")
empty = Status.Empty
```

A payload-free variant is a value and is not called. A payload variant is called with its exact payload shape:

Every payload slot is owned. A declaration such as `Failed(str)` therefore
has the constructor contract `Failed(own str)`, and a named `Ready(value:
T)` slot is constructed as `Ready(value: own T)`. This rule also applies to
builtin variants such as `Option.Some(own T)`, `Result.Ok(own T)`, and
`Result.Err(own E)`.

- positional variants accept positional arguments in declaration order; a single positional payload also accepts `value=`
- named variants accept either positional arguments in declaration order or their declared payload names
- every payload must be supplied exactly once
- unknown, duplicate, missing, or excess payload arguments are rejected
- each payload expression must have the exact substituted payload type
- a non-copy payload expression is consumed by its `own` payload slot

Do not mix positional and named construction styles in one variant call. User-defined named variants should use their declared names for clarity; multi-payload positional variants cannot be constructed with arbitrary named arguments.

Named payload expressions evaluate in the order written at the call site.
Their captured results then bind by payload name to declaration-order payload
slots; declaration order does not reorder expression evaluation. Pattern
payload positions continue to correspond to that declaration order.

## Generic Construction And Inference

Explicit specialization fixes generic arguments:

```python
ok = Result[int32, str].Ok(7)
missing = Option[str].None
```

Generic enum arguments may instead be inferred from payloads or an expected annotation, argument, or return type:

```python
ok: Result[int32, str] = Result.Ok(7)
missing: Option[str] = Option.None
```

Every type parameter must resolve. A payload-free generic variant such as `Option.None` usually needs an expected type or explicit specialization because it carries no value from which to infer `T`.

Bare builtin constructor names such as `Some(...)`, `Ok(...)`, `Err(...)`, or `None` are accepted only where the expected enum identity is unambiguous. Qualified constructors are the normative reference style.

## Copy And Move Behavior

A user enum is copyable when every payload type declared by every variant is statically copyable. Otherwise the enum is a move type. This classification is structural across all variants, not based on the variant held at runtime.

`Option[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` follow the
same payload-copy rule. `TaskResult[T]`, `SelectOutcome[Q, T]`, `WaitAny[T]`,
and `WaitAll[T]` remain move outcome types in Aura 0.2 even for copy
payloads. An unconstrained generic payload is not assumed copyable. See
[Types](/manual/types#copy-and-move-categories).

## Statement Matches

Statement-form `match` executes a statement suite:

```python
match ready:
    case Status.Ready(count):
        print(count)
    case Status.Failed(message):
        print(message)
    case Status.Empty:
        print("empty")
```

The scrutinee is evaluated exactly once. Arms are considered in source order and only the selected arm executes. Payload subpatterns are positional even when construction uses named payload arguments: they correspond to payload declaration order.

An enum match must cover every variant and all relevant nested payload patterns, or finish with `_`:

```python
match ready:
    case Status.Ready(count):
        print(count)
    case _:
        print("not ready")
```

The wildcard binds nothing. An unguarded wildcard may appear only once and
must be the final arm. A guarded wildcard may appear earlier because its guard
can be false. Duplicate and provably unreachable unguarded arms are rejected.

## Match Expressions

A match expression produces a value:

```python
def status_label(status: own Status) -> str:
    return match status:
        case Status.Ready(count):
            f"ready: {count}"
        case Status.Failed(message):
            message
        case Status.Empty:
            "empty"
```

Each arm contains exactly one expression, not a general statement suite. All arm results must have one compatible exact type, using an expected surrounding type when available. Only the selected arm expression is evaluated.

Expression arms may also use the inline grammar `case Pattern: expression`; statement-match arms must put a suite on following indented lines. See [Grammar](/manual/grammar#match-expressions) for the exact layout forms.

## Pattern Forms

At the top level of a match arm, Aura 0.2 supports:

- an enum variant pattern for an enum scrutinee
- a recursively nested fixed-arity tuple pattern for a tuple scrutinee
- a supported literal pattern for a scalar scrutinee
- a lowercase name that binds the complete scrutinee
- `_`

`case value:` is an unguarded catch-all, binds the complete scrutinee, and must
be the final arm. `case value if condition:` makes that binding visible to the
guard and body; because the condition may be false, another unguarded catch-all
is still required for an open or otherwise uncovered domain. Use `_` when the
complete value is intentionally ignored.

Variant payload patterns must match the exact payload arity. A payload-carrying variant must bind or structurally match all payload positions; a payload-free variant accepts no subpatterns. Nested variant patterns are supported when their payload types are enums.

Tuple patterns use `(left, right)` or singleton `(value,)` syntax and may nest
other supported patterns:

    match ((1, 2), true):
        case ((left, right), flag):
            print(left + right)
            print(flag)

The tuple arity and recursive shape must match exactly. Empty tuple patterns,
multi-element trailing commas, and rest/star patterns are rejected.

Each pattern binding is local to that arm and cannot shadow a name already visible there. `_` never introduces a binding. See [Names And Scopes](/manual/names-and-scopes#pattern-scope).

## Guards And Alternative Patterns

Add `if` after a pattern when structural matching needs one exact Boolean
condition. Join alternatives with `|` when one arm accepts several shapes:

    match response:
        case Result.Ok(value) if value > 0:
            print("positive")
        case Result.Ok(value) | Result.Err(value):
            print(value)

Alternatives are probed from left to right. The first structural match
supplies the guard and body bindings, and the guard runs once. Every
alternative must bind exactly the same names with identical types and
capabilities. Duplicate or subsumed alternatives are rejected.

A guard must have exactly type `bool`. A false guard continues with the next
arm. A guarded arm contributes no exhaustiveness coverage, including
`case _ if condition`. A trap or propagated failure remains primary.

`match own` probes before extracting a non-copy payload. Candidate bindings
may be inspected in the guard but cannot move until the guard commits the arm.
`match mut` publishes guard mutations before false continuation, `try`
propagation, or trap cleanup, so later arms and cleanup observe the update.

## Qualified And Short Variant Patterns

The fully qualified style is always valid:

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

When the scrutinee type supplies one unambiguous enum identity, the enum prefix may be omitted:

```python
match result:
    case Ok(value):
        print(value)
    case Err(message):
        print(message)
```

Use the qualified form in public examples and reference material when ambiguity is possible. A qualified pattern must name the scrutinee's actual enum and an existing variant.

## Match Capabilities

`match own` consumes a non-copy scrutinee place and yields owned payload
bindings:

```python
def main():
    result: Result[str, str] = Result.Ok("hello")

    match own result:
        case Result.Ok(message):
            print(message)
        case Result.Err(error):
            print(error)
```

Use bare `match` to retain the scrutinee and expose shared non-copy payload
bindings:

```python
result: Result[str, str] = Result.Ok("hello")

match result:
    case Result.Ok(message):
        print(message)
    case Result.Err(error):
        print(error)

print("result is still owned")
```

`match mut` requires a mutable place scrutinee. It gives mutable-borrowed
payload bindings and reconstructs/writes the enum value back on normal arm
exit, `return`, `break`, `continue`, and `try` propagation. Overlapping nested
mutable matches are rejected. A payload binding becomes stale if the exact
matched place, its root, or an ancestor field is reassigned; a proven-disjoint
sibling-field write remains valid.

For tuple patterns, `match own` consumes a non-copy tuple as one whole value
and gives owned leaf bindings. Bare `match` retains the tuple and gives
shared leaf provenance. `match mut` with a tuple pattern is rejected;
the minimal tuple surface has no recursive reconstruction and writeback rule.

Borrowed payloads cannot be moved as owned values. Copy payloads are ordinary copies. The complete place and provenance rules are in [Ownership And Borrowing](/manual/ownership-and-borrowing#borrowed-pattern-matching).

## Literal Matches

Literal patterns are supported for `bool`, integer, floating-point, and `str` scrutinees:

```python
match code:
    case 200:
        print("ok")
    case 404:
        print("missing")
    case _:
        print("other")
```

The literal must have the scrutinee's exact scalar type after contextual literal checking. Duplicate literals and arms after a covering wildcard are unreachable and rejected.

Boolean matching is exhaustive when both `true` and `false` are covered by
unguarded arms. Integer, floating-point, and string domains are open-ended and
therefore require a final unguarded wildcard. Class patterns are deferred;
use an explicit enum/tag representation or a wildcard and ordinary code.

## Builtin Enum Shapes

These builtin generic enums are available without a module prefix. The table
shows constructor contracts, so `own` makes their implicit payload ownership
visible; enum declarations themselves continue to write only the payload type:

| Type | Variants |
| --- | --- |
| `Option[T]` | `Some(value: own T)`, `None` |
| `Result[T, E]` | `Ok(value: own T)`, `Err(error: own E)` |
| `SendError[T]` | `Closed(value: own T)`, `Cancelled(value: own T)`, `TimedOut(value: own T)`, `Full(value: own T)` |
| `QueueReceive[T]` | `Item(value: own T)`, `Closed`, `TimedOut`, `Cancelled` |
| `TaskResult[T]` | `Ready(value: own T)`, `Error(message: own str)`, `TimedOut`, `Cancelled` |
| `SelectOutcome[Q, T]` | `Queue(index: own int64, outcome: own QueueReceive[Q])`, `Task(index: own int64, outcome: own TaskResult[T])`, `Deadline(index: own int64)`, `Cancelled` |
| `WaitAny[T]` | `Ready(index: own int64, value: own T)`, `Error(index: own int64, message: own str)`, `TimedOut`, `Cancelled` |
| `WaitAll[T]` | `Ready(values: own list[T])`, `Error(index: own int64, message: own str)`, `TimedOut`, `Cancelled` |

Module-qualified builtin enums are specified by their API chapters:

| Type | Reference |
| --- | --- |
| `io.Error` | [I/O Module](/manual/io) |
| `process.ExitStatus`, `process.Wait`, `process.RestartPolicy` | [Process Module](/manual/process) |
| `process.Error`, `process.SupervisorEvent`, `process.SupervisorWait` | [Process Module](/manual/process) |

Treat every documented timeout, cancellation, closure, and error variant as semantically distinct. Use `_` only when all remaining outcomes genuinely share one policy.

## Grammar

The normative enum declaration, generic parameter, variant payload,
construction, statement-match, expression-match, pattern, and match-capability
productions are in [Grammar](/manual/grammar#enums),
[Grammar](/manual/grammar#patterns-and-statement-matches), and
[Grammar](/manual/grammar#match-expressions). Payload-free variants omit
parentheses. Variant declarations use either positional or named payloads and
cannot mix the two forms in one variant.

## Typing Rules

Enums are nominal, substitutions are invariant, and every payload has one
exact declared type after generic substitution. A constructor must identify
one existing variant and bind its complete payload shape. Generic arguments
come from explicit specialization, payloads, or expected type; every parameter
must resolve and satisfy its bounds. A match pattern must agree with the
scrutinee type and payload arity. Enum and Boolean matches are exhaustive;
open scalar literal domains require a final wildcard. Match-expression arms
produce one compatible exact result type.

## Runtime Semantics

An enum value stores one variant and its payloads. Constructor payload
expressions evaluate in source order. For named construction, captured results
then bind by payload name to declaration-order slots. Equality compares nominal
enum identity, variant, and payload values. A match evaluates its scrutinee exactly once,
tests arms in source order, and executes only the first matching arm. A match
expression evaluates only its selected result expression. `match mut`
reconstructs and writes the selected enum value back to its mutable place on
every arm exit.

## Ownership And Evaluation Order

Every variant payload is an owned destination. `match own` consumes a non-copy
scrutinee and gives owned non-copy payload bindings. Bare `match` retains the
scrutinee and exposes shared payload borrows; `match mut`
requires one exclusive mutable place and exposes mutable payload borrows.
Copy payloads copy normally. Pattern bindings are arm-local, and reassigning a
matched place or ancestor invalidates dependent mutable bindings while a
proven-disjoint sibling write does not. Aura performs no hidden payload clone.

## Diagnostics

`AU1101` reports malformed enum, variant, match, arm, guard, or or-pattern syntax.
`AU2001` reports unknown enum types, variants, and payload types. `AU2002`
covers generic inference or bounds, constructor/payload type mismatch,
literal-pattern type mismatch, a non-Boolean guard, alternative binding type
mismatch, and incompatible match-expression results.
`AU2004` reports invalid variant-constructor argument binding. `AU2999` covers
duplicate variants, invalid payload shapes, missing or unreachable arms,
non-exhaustive matches, mismatched alternative bindings, duplicate/subsumed
alternatives, class patterns, unsupported pattern forms, and remaining enum/match
rejections. `AU3001` reports use after `match own`, a payload move, or moving
an owned candidate before its guard commits.
`AU3002` reports moving through a shared match, overlapping mutable matches, or
requiring a mutable match place.
`AU3003` reports mutation or reassignment through an immutable enum/payload
place.

Operations in the selected arm retain their runtime code: `AU4001` for a
general trap, `AU4002` for arithmetic overflow or underflow, `AU4003` for a
bounds or lookup violation, `AU4004` for a zero divisor, and `AU4005` for a
resource or I/O failure.

## Backend Support

User and builtin generic enums, structural enum equality, construction and
inference, statement and expression matches, exhaustiveness, nested patterns,
short variants, scalar literal patterns, guards, or-patterns,
top-level catch-all bindings, owned/shared/mutable matching, and
borrowed matching are implemented for MIR execution and direct native
generation. Both backends receive the same checked arm decision tree and are
forced to agree on selected arms, payload values, writeback, and primary
diagnostics.

## Limits And Implementation-Defined Behavior

Aura has no range/rest patterns, named-payload patterns, class/collection
destructuring, arbitrary predicate pattern, Duration/f-string pattern, or inline suite for
statement matches. Expression arms contain exactly one expression.
`TaskResult`, `SelectOutcome`, `WaitAny`, and `WaitAll` remain move outcome
types regardless of copy payloads. Scrutinee and arm order, exhaustiveness,
payload order, and borrowed-match writeback are language-defined rather than
implementation-defined.

## Status

Nominal and generic enums, positional and named payloads, qualified and
contextual builtin construction, structural copy/move classification,
statement and expression matches, exhaustiveness, nested enum patterns, scalar
literal patterns, top-level catch-all bindings, wildcards, short variants, and borrowed matching are
implemented for the post-Phase 1.5 surface. Match expressions, like every
expression, produce owned results; a non-copy result must come from an owned
source. Tuple patterns are implemented under Accepted ADR-0026. Guards and
or-patterns are implemented under Accepted ADR-0049. Class/collection
destructuring beyond the tuple kernel and arbitrary predicate patterns are
unavailable.
