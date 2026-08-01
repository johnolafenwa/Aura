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
    Failure(String)

ok = ParseResult.Success(42)
bad = ParseResult.Failure("invalid input")
```

Variant payloads are owned constructor positions. `Failure(String)` therefore
acts like `Failure(own String)`, and the same is true of builtins such as
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
wrapped = Result[int32, String].Ok(7)
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
result: Result[String, String] = Result.Ok("ok")

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
result: Result[String, String] = Result.Ok("ok")

match result:
    case Ok(value):
        print(value.clone())    # value is a borrowed String
    case Err(message):
        print(message)

# result is still valid here
```

Use `match mut` when you need to modify the matched value:

```python
mut result: Result[String, String] = Result.Ok("hello")
match mut result:
    case Ok(msg):
        pass    # msg is mut String
    case Err(e):
        pass
```

The scrutinee may be a field such as `holder.state`. Reassigning that field, `holder`, or an ancestor field makes its payload bindings stale, while changing a separate sibling field is allowed. See [examples/enums/match_borrow_mut_fields.au](../examples/enums/match_borrow_mut_fields.au).

See [examples/enums/match_borrow.au](../examples/enums/match_borrow.au).

## Literal Match Patterns

You can also match on literal values of `bool`, integer, and `String`:

```python
def describe_number(value: int32) -> String:
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
def describe_flag(flag: bool) -> String:
    match flag:
        case true:
            return "yes"
        case false:
            return "no"
```

Integer and `String` matches always need a final wildcard arm because the domain is open-ended.

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
