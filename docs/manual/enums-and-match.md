# Enums And Pattern Matching

An enum is a nominal sum type. Each value holds exactly one declared variant. When that variant has payloads, the value holds one value for each payload position. A `match` evaluates its scrutinee once, selects the first matching arm, and binds payload values for that arm.

Aura uses enums for user data and for maintained runtime outcomes: `Result`, `Lookup`, `Poll`, queue operations, task waits, process status, supervisor events, and I/O errors. Ordinary absence is not an enum. It is the union `T | None`, described in [Types](/manual/types#optional-and-result-types).

## Union Type Patterns

Match a union by its direct member types:

```aura
def main():
    mut value: int64 | str | None = 41
    match mut value:
        case int64 as number:
            number += 1
            print(number)
        case str as text:
            print(text)
        case None:
            print("missing")
    print(value)
```

This prints `42` twice.

`case Type as name` selects exactly one normalized member, after transparent alias expansion.

- An alias for several members cannot stand for a single type arm.
- A type parameter cannot stand for a type arm either. `case V as inner` is rejected with `AU2013`, because `V` is not proved disjoint from the other arms. Use `case None` and a catch-all.
- A matching type arm is irrefutable when the scrutinee type has collapsed to one member.
- Type patterns may also appear inside nominal enum payload patterns.

### Coverage

- An unguarded type arm covers its member. Guards contribute no exhaustiveness coverage.
- A final `_` or name covers the remainder.
- Missing members, duplicate coverage, unreachable type arms, and nonmembers are rejected with `AU2013`.
- Or-patterns must bind identical names, types, and capabilities.
- A bare union cannot be matched directly against a member literal. Select the type first, then use a guard or a nested match for the literal.

### Capabilities

- A bare `match` borrows non-Copy payloads and exposes Copy values as ordinary values.
- `match mut` gives mutable payload bindings without another `mut` after `as`. A replacement must keep the member type.
- `match own` consumes the union and moves the selected payload only after its guard succeeds.
- Failed patterns and guards release their temporary views.
- Payload views are local to the arm. `return`, `break`, and `continue` release them before cleanup.
- Shared and mutable matches lock the original source for the arm, even when the selected payload is Copy.
- Replacing the whole union to change its tag is allowed once the conflicting arm access ends.

### Union Properties

- A union is Copy when every member is Copy. Otherwise it moves.
- `.clone()` is available when every member is Copy or clones itself. Cloning copies the active payload.
- A union crosses a task boundary when every member is Transfer.
- Two values of one union are equal when their active members agree and their payloads are equal.
- A union compares symmetrically with a value that injects as one of its members. See [equality](/manual/expressions#arithmetic-and-comparison).
- Printing a union renders its active payload, or `None`, without a tag.
- Unions have no ordering.

### No Builtin Option

Aura has no builtin `Option` type, `Some` constructor, or `T?` suffix. Without a user declaration, `Option[T]` reports the ordinary `AU2001` unknown type, and `T?` reports the ordinary `AU1101` parse error. A user may declare an ordinary enum named `Option`, and it receives no builtin behavior.

## Conditional Narrowing

`value is None` and `value is not None` test a union's `None` member without a match. When the tested operand is a stable place, the checker records a narrowing fact for each branch the test selects:

```aura
def describe(value: str | None):
    if value is None:
        print("missing")
    else:
        print(value.len())

def main():
    describe(None)
    describe("aura")
    mut local: int64 | None = 7
    if local is not None:
        local += 1
        print(local)
    local = None
    if local is None:
        print("cleared")
```

This prints `missing`, `4`, `8`, and `cleared`. Where `value is not None` holds, `value` has the effective type `str`. Where it fails, `value` has the effective type `None`.

Only `is None` and `is not None` narrow. `==` and `!=` compare values. A `match` arm narrows its own binding instead.

### Stable Places

A stable place is one of:

- an owned or `mut` local
- a parameter or receiver
- a fixed class field or tuple position reached through one of those
- a view

An index, a dictionary lookup, a call result, or any other temporary may be tested, but no fact attaches to a later evaluation. Bind the value first.

Narrowing never changes the capability of the place:

- A shared parameter gives shared payload access.
- A `mut` place or `view mut` gives mutable access.
- An owned local may be consumed as the member, which consumes the whole union.

### How Facts Flow

- Facts flow through `not`, parentheses, and short-circuit `and` and `or`.
- Both operands of `and` hold in its `true` branch. Both operands of `or` fail in its `false` branch.
- The right operand is checked under the left operand's fact.
- A branch that ends with `return`, `break`, or `continue` leaves its complement on the continuing path. So `if value is None: return` narrows the rest of the function, and `if value is None: continue` narrows the rest of the loop iteration.
- Where paths join, only facts present on every reachable path survive.
- A fact established before a `while` or `for` loop survives into the loop only when no iteration can invalidate it.
- A `while` condition's own fact is re-established before every iteration.

### When Facts End

A fact ends when the place or an enclosing place is assigned, matched with `match mut`, or passed to a call with `mut` access. A later member use through that place is rejected with `AU2014`. The diagnostic labels the test and the invalidation. Test the current value again to fix it.

Moving the place, including with `match own`, leaves it moved as usual, so a later use reports `AU3001`.

Two writes keep the fact, because neither can change the member:

- writing through the narrowed payload, such as `local += 1` above
- changing a proven-disjoint sibling field

A narrowed union with more than two members keeps its remaining member set for match coverage. After `if value is None: return`, a `match value` over `int64 | str | None` is exhaustive with only `int64` and `str` arms.

## Enum Declarations

```aura
enum Status:
    Ready(count: int32)
    Failed(str)
    Empty
```

A variant has one of three shapes:

- no payload, written without parentheses: `Empty`
- positional payloads, written as types: `Failed(str)` or `Pair(int32, int32)`
- named payloads, written as `name: Type`: `Ready(count: int32)`

One variant cannot mix positional and named payloads. Empty parentheses do not declare a payload-free variant, so omit them. Variant names must be unique within the enum. Every payload type must exist and have the correct arity.

Enums may be generic and bounded:

```aura
enum Load[T: Named]:
    Ready(T)
    Failed(message: str)
    Empty
```

Type parameters must be unique, substitutions are invariant, and every bound must be satisfied. See [Generics And Traits](/manual/generics-and-traits).

An enum is private to its defining module unless it is declared `public enum`. Variants have no visibility modifiers of their own. Importing a public enum exposes its variant constructors. Construction and matching type-check only when the imported payload types are usable in the importing context.

The full declaration and pattern syntax is in [Grammar](/manual/grammar#enums) and [Grammar](/manual/grammar#patterns-and-statement-matches).

## Construction

Use the enum type and the variant name:

```aura
ready = Status.Ready(count=3)
failed = Status.Failed("disk full")
empty = Status.Empty
```

A payload-free variant is a value and is not called. A payload variant is called with its exact payload shape.

Every payload slot is owned. A declaration such as `Failed(str)` has the constructor contract `Failed(own str)`, and a named `Ready(value: T)` slot is constructed as `Ready(value: own T)`. The same holds for builtin variants such as `Lookup.Found(own T)`, `Poll.Ready(own T)`, `Result.Ok(own T)`, and `Result.Err(own E)`.

Construction rules:

- Positional variants accept positional arguments in declaration order. A single positional payload also accepts `value=`.
- Named variants accept positional arguments in declaration order, or their declared payload names.
- Every payload must be supplied exactly once.
- Unknown, duplicate, missing, and excess payload arguments are rejected.
- Each payload expression must have the exact substituted payload type.
- A non-copy payload expression is consumed by its `own` payload slot.

Do not mix positional and named arguments in one variant call. Use the declared names for user-defined named variants, for clarity. A positional variant with several payloads cannot be constructed with arbitrary named arguments.

Named payload expressions evaluate in the order written at the call site. Their results then bind by name to the payload slots in declaration order. Declaration order does not reorder evaluation. Pattern payload positions follow declaration order.

## Generic Construction And Inference

Explicit specialization fixes the generic arguments:

```aura
ok = Result[int32, str].Ok(7)
missing = Lookup[str].Missing
```

The checker may instead infer generic enum arguments from payloads, or from an expected annotation, argument, or return type:

```aura
ok: Result[int32, str] = Result.Ok(7)
missing: Lookup[str] = Lookup.Missing
```

Every type parameter must resolve. A payload-free generic variant such as `Lookup.Missing` has no value to infer `T` from, so it usually needs an expected type or explicit specialization.

Bare builtin constructor names such as `Found(...)`, `Missing`, `Ok(...)`, and `Err(...)` are accepted only where the expected enum identity is unambiguous. Qualified constructors are the normative reference style. A bare `None` is never an enum variant. It is the unit value, or the absence member of an expected `T | None` union.

## Copy And Move Behavior

A user enum is copyable when every payload type of every variant is statically copyable. Otherwise the enum is a move type. The classification is structural across all variants. It does not depend on the variant held at runtime.

`Lookup[T]`, `Poll[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` follow the same payload-copy rule. `TaskResult[T]`, `SelectOutcome[Q, T]`, `WaitAny[T]`, and `WaitAll[T]` are move outcome types in Aura 0.3, even with copy payloads. An unconstrained generic payload is not assumed copyable. See [Types](/manual/types#copy-and-move-categories).

## Statement Matches

A statement-form `match` runs a statement suite:

```aura
match ready:
    case Status.Ready(count):
        print(count)
    case Status.Failed(message):
        print(message)
    case Status.Empty:
        print("empty")
```

The scrutinee is evaluated exactly once. Arms are tried in source order, and only the selected arm runs. Payload subpatterns are positional even when construction uses named payload arguments. They follow payload declaration order.

An enum match must cover every variant and every relevant nested payload pattern, or end with `_`:

```aura
match ready:
    case Status.Ready(count):
        print(count)
    case _:
        print("not ready")
```

- The wildcard binds nothing.
- An unguarded wildcard may appear only once, and it must be the final arm.
- A guarded wildcard may appear earlier, because its guard can be false.
- Duplicate and provably unreachable unguarded arms are rejected.

## Match Expressions

A match expression produces a value:

```aura
def status_label(status: own Status) -> str:
    return match status:
        case Status.Ready(count):
            f"ready: {count}"
        case Status.Failed(message):
            message
        case Status.Empty:
            "empty"
```

Each arm contains exactly one expression, not a statement suite. All arm results must have one compatible exact type, using the surrounding expected type when there is one. Only the selected arm's expression is evaluated.

Expression arms may also use the inline form `case Pattern: expression`. Statement-match arms must put their suite on the following indented lines. See [Grammar](/manual/grammar#match-expressions) for the exact layout forms.

## Pattern Forms

At the top level of a match arm, Aura 0.3 supports:

- an enum variant pattern for an enum scrutinee
- a recursively nested fixed-arity tuple pattern for a tuple scrutinee
- a supported literal pattern for a scalar scrutinee
- a lowercase name that binds the complete scrutinee
- `_`

`case value:` is an unguarded catch-all. It binds the complete scrutinee and must be the final arm. In `case value if condition:`, the binding is visible to the guard and the body. The condition may be false, so an open or otherwise uncovered domain still needs another unguarded catch-all. Use `_` when you intend to ignore the complete value.

Variant payload patterns must match the exact payload arity. A payload-carrying variant must bind or structurally match every payload position. A payload-free variant accepts no subpatterns. Nested variant patterns are supported when their payload types are enums.

Tuple patterns use `(left, right)` or the singleton form `(value,)`, and may nest other supported patterns:

    match ((1, 2), true):
        case ((left, right), flag):
            print(left + right)
            print(flag)

The tuple arity and recursive shape must match exactly. Empty tuple patterns, trailing commas after several elements, and rest or star patterns are rejected.

Each pattern binding is local to its arm and cannot shadow a name already visible there. `_` never introduces a binding. See [Names And Scopes](/manual/names-and-scopes#pattern-scope).

## Guards And Alternative Patterns

Add `if` after a pattern when structural matching needs one exact Boolean condition. Join alternatives with `|` when one arm accepts several shapes:

    match response:
        case Result.Ok(value) if value > 0:
            print("positive")
        case Result.Ok(value) | Result.Err(value):
            print(value)

Alternatives:

- Alternatives are tried from left to right.
- The first structural match supplies the bindings for the guard and body, and the guard runs once.
- Every alternative must bind exactly the same names, with identical types and capabilities.
- Duplicate or subsumed alternatives are rejected.

Guards:

- A guard must have exactly type `bool`.
- A false guard continues with the next arm.
- A guarded arm contributes no exhaustiveness coverage. This includes `case _ if condition`.
- A trap or propagated failure remains primary.
- `match own` probes before it extracts a non-copy payload. The guard may inspect candidate bindings, but they cannot move until the guard commits the arm.
- `match mut` publishes guard mutations before false continuation, `try` propagation, or trap cleanup. Later arms and cleanup see the update.

## Qualified And Short Variant Patterns

The fully qualified style is always valid:

```aura
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

When the scrutinee type supplies one unambiguous enum identity, you may omit the enum prefix:

```aura
match result:
    case Ok(value):
        print(value)
    case Err(message):
        print(message)
```

Use the qualified form in public examples, and in reference material where ambiguity is possible. A qualified pattern must name the scrutinee's actual enum and an existing variant.

## Match Capabilities

`match own` consumes a non-copy scrutinee place and gives owned payload bindings:

```aura
def main():
    result: Result[str, str] = Result.Ok("hello")

    match own result:
        case Result.Ok(message):
            print(message)
        case Result.Err(error):
            print(error)
```

A bare `match` keeps the scrutinee and gives shared bindings for non-copy payloads:

```aura
result: Result[str, str] = Result.Ok("hello")

match result:
    case Result.Ok(message):
        print(message)
    case Result.Err(error):
        print(error)

print("result is still owned")
```

`match mut` requires a mutable place as the scrutinee. It gives mutably borrowed payload bindings. It rebuilds the enum value and writes it back on normal arm exit, `return`, `break`, `continue`, and `try` propagation.

- Overlapping nested mutable matches are rejected.
- A payload binding becomes stale if the exact matched place, its root, or an ancestor field is reassigned.
- A write to a proven-disjoint sibling field keeps the binding valid.

With tuple patterns:

- `match own` consumes a non-copy tuple as one whole value and gives owned leaf bindings.
- A bare `match` keeps the tuple and gives shared leaf provenance.
- `match mut` is rejected, because tuples have no rule for recursive reconstruction and writeback.

A borrowed payload cannot be moved as an owned value. Copy payloads are ordinary copies. The full place and provenance rules are in [Ownership And Borrowing](/manual/ownership-and-borrowing#borrowed-pattern-matching).

## Literal Matches

Literal patterns work for `bool`, integer, floating-point, and `str` scrutinees:

```aura
match code:
    case 200:
        print("ok")
    case 404:
        print("missing")
    case _:
        print("other")
```

The literal must have the scrutinee's exact scalar type after contextual literal checking. Duplicate literals and arms after a covering wildcard are unreachable and rejected.

A Boolean match is exhaustive when unguarded arms cover both `true` and `false`. Integer, floating-point, and string domains are open-ended, so they need a final unguarded wildcard.

Class patterns are not implemented. Use an explicit enum or tag representation, or a wildcard and ordinary code.

## Builtin Enum Shapes

These builtin generic enums are available without a module prefix. The table shows constructor contracts, so `own` makes the implicit payload ownership visible. Enum declarations themselves write only the payload type.

| Type | Variants |
| --- | --- |
| `Lookup[T]` | `Found(value: own T)`, `Missing` |
| `Poll[T]` | `Ready(value: own T)`, `Unavailable` |
| `Result[T, E]` | `Ok(value: own T)`, `Err(error: own E)` |
| `SendError[T]` | `Closed(value: own T)`, `Cancelled(value: own T)`, `TimedOut(value: own T)`, `Full(value: own T)` |
| `QueueReceive[T]` | `Item(value: own T)`, `Closed`, `TimedOut`, `Cancelled` |
| `TaskResult[T]` | `Ready(value: own T)`, `Error(message: own str)`, `TimedOut`, `Cancelled` |
| `SelectOutcome[Q, T]` | `Queue(index: own int64, outcome: own QueueReceive[Q])`, `Task(index: own int64, outcome: own TaskResult[T])`, `Deadline(index: own int64)`, `Cancelled` |
| `WaitAny[T]` | `Ready(index: own int64, value: own T)`, `Error(index: own int64, message: own str)`, `TimedOut`, `Cancelled` |
| `WaitAll[T]` | `Ready(values: own list[T])`, `Error(index: own int64, message: own str)`, `TimedOut`, `Cancelled` |

`list.get`, `dict.get`, and `dict.remove` return `Lookup[T]`, so a missing entry stays distinct from a present `None` payload. `Lookup.Found(None)` differs from `Lookup.Missing`. `Queue.poll` and `Task.poll` return `Poll[T]`, with the same distinction between `Poll.Ready(None)` and `Poll.Unavailable`. Their payloads are owned. Copy, clone, equality, and Transfer follow the payload rules above after substitution.

The API chapters specify the module-qualified builtin enums:

| Type | Reference |
| --- | --- |
| `io.Error` | [I/O Module](/manual/io) |
| `process.ExitStatus`, `process.Wait`, `process.RestartPolicy` | [Process Module](/manual/process) |
| `process.Error`, `process.SupervisorEvent`, `process.SupervisorWait` | [Process Module](/manual/process) |

Treat every documented timeout, cancellation, closure, and error variant as a distinct outcome. Use `_` only when all remaining outcomes share one policy.

## Grammar

The normative productions for enum declarations, generic parameters, variant payloads, construction, statement matches, expression matches, patterns, and match capabilities are in [Grammar](/manual/grammar#enums), [Grammar](/manual/grammar#patterns-and-statement-matches), and [Grammar](/manual/grammar#match-expressions). Payload-free variants omit parentheses. A variant declaration uses either positional or named payloads, never both.

## Typing Rules

- Enums are nominal, and substitutions are invariant.
- Every payload has one exact declared type after generic substitution.
- A constructor must identify one existing variant and bind its complete payload shape.
- Generic arguments come from explicit specialization, payloads, or the expected type. Every parameter must resolve and satisfy its bounds.
- A match pattern must agree with the scrutinee type and payload arity.
- Enum and Boolean matches must be exhaustive. Open scalar literal domains require a final wildcard.
- Match-expression arms produce one compatible exact result type.

## Runtime Semantics

An enum value stores one variant and its payloads. Constructor payload expressions evaluate in source order. For named construction, the results then bind by payload name to the slots in declaration order. Equality compares the nominal enum identity, the variant, and the payload values.

A match evaluates its scrutinee exactly once, tests arms in source order, and runs only the first matching arm. A match expression evaluates only its selected result expression. `match mut` rebuilds the selected enum value and writes it back to its mutable place on every arm exit.

## Ownership And Evaluation Order

- Every variant payload is an owned destination.
- `match own` consumes a non-copy scrutinee and gives owned bindings for non-copy payloads.
- A bare `match` keeps the scrutinee and exposes shared payload borrows.
- `match mut` requires one exclusive mutable place and exposes mutable payload borrows.
- Copy payloads copy normally.
- Pattern bindings are local to their arm.
- Reassigning a matched place or an ancestor invalidates dependent mutable bindings. A write to a proven-disjoint sibling does not.
- Aura performs no hidden payload clone.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed enum, variant, match, arm, guard, or or-pattern syntax. |
| `AU2001` | Unknown enum type, variant, or payload type. |
| `AU2002` | Generic inference or bound failure. Constructor or payload type mismatch. Literal-pattern type mismatch. A non-Boolean guard. Alternative binding type mismatch. Incompatible match-expression results. |
| `AU2004` | Invalid variant-constructor argument binding. |
| `AU2013` | Union type-arm coverage: missing members, duplicate coverage, unreachable type arms, nonmembers, and type arms that are not one member. |
| `AU2014` | A member use through a place whose narrowing fact was invalidated. |
| `AU2999` | Duplicate variants, invalid payload shapes, missing or unreachable arms, non-exhaustive matches, mismatched alternative bindings, duplicate or subsumed alternatives, class patterns, unsupported pattern forms, and other enum and match rejections. |
| `AU3001` | Use after `match own`, after a payload move, or after moving an owned candidate before its guard commits. |
| `AU3002` | Moving through a shared match, overlapping mutable matches, or a `match mut` scrutinee that is not a mutable place. |
| `AU3003` | Mutation or reassignment through an immutable enum or payload place. |

Operations in the selected arm keep their runtime codes:

| Code | Cause |
| --- | --- |
| `AU4001` | General trap. |
| `AU4002` | Arithmetic overflow or underflow. |
| `AU4003` | Bounds or lookup violation. |
| `AU4004` | Zero divisor. |
| `AU4005` | Resource or I/O failure. |

## Backend Support

MIR execution and direct native generation both implement:

- user and builtin generic enums
- structural enum equality
- construction and inference
- statement and expression matches, with exhaustiveness
- nested patterns and short variants
- scalar literal patterns
- guards and or-patterns
- top-level catch-all bindings
- owned, shared, mutable, and borrowed matching

Both backends receive the same checked arm decision tree. They must agree on selected arms, payload values, writeback, and primary diagnostics.

## Limits And Implementation-Defined Behavior

- Aura has no range or rest patterns, named-payload patterns, class or collection destructuring, arbitrary predicate patterns, `Duration` or f-string patterns, or inline suites for statement matches.
- Expression arms contain exactly one expression.
- `TaskResult`, `SelectOutcome`, `WaitAny`, and `WaitAll` are move outcome types regardless of their payloads.
- Scrutinee and arm order, exhaustiveness, payload order, and borrowed-match writeback are language-defined, not implementation-defined.

## Status

Implemented on both backends:

- nominal and generic enums, with positional and named payloads
- qualified and contextual builtin construction
- structural copy and move classification
- statement and expression matches, with exhaustiveness
- nested enum patterns, tuple patterns, and scalar literal patterns
- top-level catch-all bindings, wildcards, and short variants
- borrowed matching
- guards, which keep candidate ownership until the arm commits, and or-patterns
- normalized union type arms, unit-member cases, singleton type arms, nested union payload patterns, and generic union members, with `AU2013` coverage diagnostics
- `is None` and `is not None` narrowing of stable places, with `AU2014` stale-narrowing diagnostics
- the union property rules: all-member Copy, clone, and Transfer, member-injected equality, and active-member trait dispatch
- mutable arms that keep the member type and source locks

Match expressions, like every expression, produce owned results. A non-copy result must come from an owned source.

Not implemented: class and collection destructuring beyond tuples, and arbitrary predicate patterns.

Design record: ADR-0026 for tuple patterns and ADR-0049 for guards and or-patterns. Both are Accepted.
