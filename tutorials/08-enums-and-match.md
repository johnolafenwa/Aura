# Enums And Match

Aurora now supports enum declarations with unit variants, single-payload variants, and exhaustive statement-form `match` over both enums and a small literal subset.

## Declaring An Enum

```python
enum TrafficLight:
    Red
    Yellow
    Green
```

Each variant belongs to the enum's namespace.

Generic enums are also supported:

```python
enum Wrapper[T]:
    Item(T)
```

## Constructing Variants

Unit variants are accessed directly:

```python
light = TrafficLight.Red
```

Payload variants are called like constructors:

```python
enum ParseResult:
    Success(int32)
    Failure(String)

ok = ParseResult.Success(42)
bad = ParseResult.Failure("bad")
```

Generic enum constructors may also use explicit type arguments on the enum name when needed:

```python
wrapped = Result[int32, String].Ok(7)
```

See [examples/enums/explicit_type_args.au](../examples/enums/explicit_type_args.au).

## Matching Exhaustively

Aurora's current `match` support requires coverage of every enum variant, either explicitly or through a final wildcard arm:

```python
def value_or_zero(result: ParseResult) -> int32:
    match result:
        case ParseResult.Success(value):
            return value
        case ParseResult.Failure(message):
            print(message)
            return 0
```

If you leave out a variant, the checker reports a non-exhaustive match error.

Wildcard arms are written with `case _:`:

```python
match light:
    case TrafficLight.Red:
        print("stop")
    case _:
        print("not red")
```

## Payload Bindings

When a case matches a payload variant, the payload name becomes available inside that arm:

```python
case ParseResult.Success(value):
    return value
```

Aurora also supports borrowed matching with `match borrow ...:` and `match borrow mut ...:`. In borrowed matches, non-copy payloads are exposed as borrowed values instead of moving the scrutinee:

```python
result: Result[String, String] = Result.Ok("ok")

match borrow result:
    case Ok(value):
        print(value.clone())
    case Err(message):
        print(message)
```

Unqualified variants like `case Ok(value):` are supported when the scrutinee already determines the enum type.

See [examples/enums/match_borrow.au](../examples/enums/match_borrow.au).

## Literal Match Patterns

Aurora also supports literal `case` arms for `bool`, integer, and `String` scrutinees:

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

Boolean matches can stay fully exhaustive without a wildcard when they cover both `true` and `false`:

```python
def describe_flag(flag: bool) -> String:
    match flag:
        case true:
            return "yes"
        case false:
            return "no"
```

Open-ended literal domains like integers and strings still need a final wildcard arm.

See [examples/control_flow/match_literals.au](../examples/control_flow/match_literals.au).

## Current Limits

The bootstrap compiler currently supports:

- non-generic and generic enums
- zero-payload and single-payload variants
- statement-form `match`
- variant patterns of the form `Enum.Variant` and `Enum.Variant(name)`
- unqualified variant patterns such as `case Ok(value):` when the scrutinee type is known
- literal patterns over `bool`, integer, and `String` scrutinees
- `match borrow value:` and `match borrow mut value:`
- wildcard patterns with `case _:`

It does not yet support:

- nested patterns
- expression-form `match`
- floating-point literal patterns
- keyword arguments for variant payload construction
- multi-payload variants

Built-in generic enums such as `Result[T, E]`, `Option[T]`, and `SendError[T]` are covered in the next chapter.

See [examples/enums/result_match.au](../examples/enums/result_match.au) and [examples/enums/wildcard_match.au](../examples/enums/wildcard_match.au).
