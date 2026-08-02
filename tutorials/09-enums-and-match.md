# Enums And Match

Enums let you define a type that can be one of several variants. Combined with `match`, they give you exhaustive pattern matching -- the compiler guarantees you handle every case.

## Declaring An Enum

```python
enum TrafficLight:
    Red
    Yellow
    Green
```

Each variant belongs to the enum's namespace: `TrafficLight.Red`, `TrafficLight.Yellow`, etc.

## Variants With Payloads

Variants can carry a single value:

```python
enum ParseResult:
    Success(int32)
    Failure(str)

ok = ParseResult.Success(42)
bad = ParseResult.Failure("invalid input")
```

Variant payloads are owned constructor positions. `Failure(str)` therefore
acts like `Failure(own str)`, and the same is true of builtins such as
`Option.Some(own T)` and `Result.Err(own E)`.

## Generic Enums

Enums can be generic:

```python
enum Wrapper[T]:
    Item(T)
    Empty
```

You can provide explicit type arguments when the compiler needs help:

```python
wrapped = Result[int32, str].Ok(7)
```

See [examples/enums/explicit_type_args.au](../examples/enums/explicit_type_args.au).

## Exhaustive `match`

Aura's `match` requires you to handle every variant. If you miss one, the compiler reports an error:

```python
def value_or_zero(result: own ParseResult) -> int32:
    match result:
        case ParseResult.Success(value):
            return value
        case ParseResult.Failure(message):
            print(message)
            return 0
```

### Wildcard Arms

Use `case _:` to match any remaining variants:

```python
match light:
    case TrafficLight.Red:
        print("stop")
    case _:
        print("not red")
```

### Payload Bindings

When a case matches a payload variant, the payload becomes a local binding:

```python
case ParseResult.Success(value):
    return value    # value is an int32 here
```

### Unqualified Variants

When the scrutinee type is already known, you can omit the enum name:

```python
result: Result[str, str] = Result.Ok("ok")

match result:
    case Ok(value):       # same as Result.Ok(value)
        print(value)
    case Err(message):    # same as Result.Err(message)
        print(message)
```

This is especially convenient with built-in enums like `Result` and `Option`.

## Borrowed Matching

Bare `match` inspects without consuming the value. Write `match own` when an
arm must receive owned payloads. This distinction matters for non-copy types
(see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md)):

```python
result: Result[str, str] = Result.Ok("ok")

match result:
    case Ok(value):
        print(value.clone())    # value is a borrowed str
    case Err(message):
        print(message)

# result is still valid here
```

Use `match mut` when you need to modify the matched value:

```python
mut result: Result[str, str] = Result.Ok("hello")
match mut result:
    case Ok(msg):
        pass    # msg is mut str
    case Err(e):
        pass
```

The scrutinee may be a field such as `holder.state`. Reassigning that field, `holder`, or an ancestor field makes its payload bindings stale, while changing a separate sibling field is allowed. See [examples/enums/match_borrow_mut_fields.au](../examples/enums/match_borrow_mut_fields.au).

See [examples/enums/match_borrow.au](../examples/enums/match_borrow.au).

## Literal Match Patterns

You can also match on literal values of `bool`, integer, and `str`:

```python
def describe_number(value: int32) -> str:
    match value:
        case 0:
            return "zero"
        case 1:
            return "one"
        case _:
            return "many"
```

Boolean matches are exhaustive when they cover both `true` and `false`:

```python
def describe_flag(flag: bool) -> str:
    match flag:
        case true:
            return "yes"
        case false:
            return "no"
```

Integer and `str` matches always need a final wildcard arm because the domain is open-ended.

See [examples/control_flow/match_literals.au](../examples/control_flow/match_literals.au).

Nested patterns, expression-form `match`, floating-point literal patterns, keyword payload arguments, and multi-payload variants are also supported:

```python
enum Inner:
    Pair(int32, int32)

enum Outer:
    Point(x: int32, y: int32)
    Wrapped(Inner)
    Empty

def describe(value: Outer) -> int32:
    return match value:
        case Outer.Point(x, y): x + y
        case Outer.Wrapped(Inner.Pair(a, b)): a * b
        case Outer.Empty: 0
```

See [examples/enums/rich_match.au](../examples/enums/rich_match.au).

## Guards And Or-Patterns

A guard adds an exact Boolean condition after structural matching. An
or-pattern lets one arm accept several structural alternatives:

```python
match code:
    case 200 | 201 if code == 201:
        print("created")
    case 200 | 201:
        print("success")
    case _:
        print("other")
```

Alternatives are tested left to right and must bind the same names with the
same types and capabilities. A false guard continues to the next arm. Guarded
arms do not make a match exhaustive, so keep an unguarded fallback when the
remaining domain is open.

In `match own`, a guard can inspect a non-copy candidate but cannot move it.
Extraction happens only after a true guard. In `match mut`, mutations made by
a guard remain visible when the guard is false or propagates a failure.

See [examples/enums/match_guards_and_or_patterns.au](../examples/enums/match_guards_and_or_patterns.au).

Expression-form `match` is not limited to `return`. It also works in binding and argument positions, and an arm value may itself be a nested block-form expression:

```python
value = match outer:
    case Outer.A: 10
    case Outer.B: 20

emit(match outer:
    case Outer.A:
        match inner:
            case Inner.X: 1
            case Inner.Y: 2
    case Outer.B: 3)
```

See [examples/enums/match_expression_positions.au](../examples/enums/match_expression_positions.au).

Built-in generic enums `Result[T, E]`, `Option[T]`, and `SendError[T]` are covered in the next chapter.

See [examples/enums/result_match.au](../examples/enums/result_match.au) and [examples/enums/wildcard_match.au](../examples/enums/wildcard_match.au).
