# Enums And Pattern Matching

Enums define nominal sum types. Each value contains exactly one declared variant and, when that variant has payloads, one value for each payload position. Pattern matching evaluates a scrutinee once, selects the first matching arm, and binds payload values for that arm.

Aurora uses enums for user data and for maintained runtime outcomes including `Option`, `Result`, queue operations, task waits, process status, supervisor events, and I/O errors.

## Enum Declarations

```python
enum Status:
    Ready(count: int32)
    Failed(String)
    Empty
```

A variant has one of three shapes:

- no payload, written without parentheses: `Empty`
- positional payloads, written as types: `Failed(String)` or `Pair(int32, int32)`
- named payloads, written as `name: Type`: `Ready(count: int32)`

One variant cannot mix positional and named payload declarations. Empty parentheses are not a payload-free declaration; omit them. Variant names must be unique within the enum. Every payload type must exist with the correct arity.

Enums may be generic and bounded:

```python
enum Load[T: Named]:
    Ready(T)
    Failed(message: String)
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

- positional variants accept positional arguments in declaration order; a single positional payload also accepts `value=`
- named variants accept either positional arguments in declaration order or their declared payload names
- every payload must be supplied exactly once
- unknown, duplicate, missing, or excess payload arguments are rejected
- each payload expression must have the exact substituted payload type
- a non-copy payload expression is consumed

Do not mix positional and named construction styles in one variant call. User-defined named variants should use their declared names for clarity; multi-payload positional variants cannot be constructed with arbitrary named arguments.

## Generic Construction And Inference

Explicit specialization fixes generic arguments:

```python
ok = Result[int32, String].Ok(7)
missing = Option[String].None
```

Generic enum arguments may instead be inferred from payloads or an expected annotation, argument, or return type:

```python
ok: Result[int32, String] = Result.Ok(7)
missing: Option[String] = Option.None
```

Every type parameter must resolve. A payload-free generic variant such as `Option.None` usually needs an expected type or explicit specialization because it carries no value from which to infer `T`.

Bare builtin constructor names such as `Some(...)`, `Ok(...)`, `Err(...)`, or `None` are accepted only where the expected enum identity is unambiguous. Qualified constructors are the normative reference style.

## Copy And Move Behavior

A user enum is copyable when every payload type declared by every variant is statically copyable. Otherwise the enum is a move type. This classification is structural across all variants, not based on the variant held at runtime.

`Option[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` follow the same payload-copy rule. `TaskResult[T]`, `WaitAny[T]`, and `WaitAll[T]` remain move outcome types in Aurora 0.1 even for copy payloads. An unconstrained generic payload is not assumed copyable. See [Types](/manual/types#copy-and-move-categories).

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

The wildcard binds nothing, may appear only once, and must be the final arm. Duplicate and provably unreachable arms are rejected.

## Match Expressions

A match expression produces a value:

```python
def status_label(status: Status) -> String:
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

At the top level of a match arm, Aurora 0.1 supports:

- an enum variant pattern for an enum scrutinee
- a supported literal pattern for a scalar scrutinee
- `_`

A lowercase binding pattern is supported inside enum payload patterns, but a top-level catch-all binding such as `case value:` is not implemented. Use `_` when the whole value is intentionally ignored.

Variant payload patterns must match the exact payload arity. A payload-carrying variant must bind or structurally match all payload positions; a payload-free variant accepts no subpatterns. Nested variant patterns are supported when their payload types are enums.

Each pattern binding is local to that arm and cannot shadow a name already visible there. `_` never introduces a binding. See [Names And Scopes](/manual/names-and-scopes#pattern-scope).

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

## Borrowed Matches

A by-value match consumes a non-copy scrutinee place and yields owned payload bindings:

```python
result: Result[String, String] = Result.Ok("hello")

match result:
    case Result.Ok(message):
        print(message)
    case Result.Err(error):
        print(error)
```

Use `match borrow` to retain the scrutinee and expose borrowed non-copy payload bindings:

```python
result: Result[String, String] = Result.Ok("hello")

match borrow result:
    case Result.Ok(message):
        print(message)
    case Result.Err(error):
        print(error)

print("result is still owned")
```

`match borrow mut` requires a mutable place scrutinee. It gives mutable-borrowed payload bindings and reconstructs/writes the enum value back to that place on normal arm exit. Overlapping nested mutable matches are rejected, and a payload binding becomes stale if the matched place is reassigned.

Borrowed payloads cannot be moved as owned values. Copy payloads are ordinary copies. The complete place and provenance rules are in [Ownership And Borrowing](/manual/ownership-and-borrowing#borrowed-pattern-matching).

## Literal Matches

Literal patterns are supported for `bool`, integer, floating-point, and `String` scrutinees:

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

Boolean matching is exhaustive when both `true` and `false` are covered. Integer, floating-point, and string domains are open-ended and therefore require a final wildcard. Classes, collections, resources, and arbitrary other values are not match scrutinee types in Aurora 0.1.

## Builtin Enum Shapes

These builtin generic enums are available without a module prefix:

| Type | Variants |
| --- | --- |
| `Option[T]` | `Some(value: T)`, `None` |
| `Result[T, E]` | `Ok(value: T)`, `Err(error: E)` |
| `SendError[T]` | `Closed(value: T)`, `Cancelled(value: T)`, `TimedOut(value: T)`, `Full(value: T)` |
| `QueueReceive[T]` | `Item(value: T)`, `Closed`, `TimedOut`, `Cancelled` |
| `TaskResult[T]` | `Ready(value: T)`, `Error(message: String)`, `TimedOut`, `Cancelled` |
| `WaitAny[T]` | `Ready(index: int32, value: T)`, `Error(index: int32, message: String)`, `TimedOut`, `Cancelled` |
| `WaitAll[T]` | `Ready(values: Vec[T])`, `Error(index: int32, message: String)`, `TimedOut`, `Cancelled` |

Module-qualified builtin enums are specified by their API chapters:

| Type | Reference |
| --- | --- |
| `io.Error` | [I/O Module](/manual/io) |
| `process.ExitStatus`, `process.Wait`, `process.RestartPolicy` | [Process Module](/manual/process) |
| `process.Error`, `process.SupervisorEvent`, `process.SupervisorWait` | [Process Module](/manual/process) |

Treat every documented timeout, cancellation, closure, and error variant as semantically distinct. Use `_` only when all remaining outcomes genuinely share one policy.
