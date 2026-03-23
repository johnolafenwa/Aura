# Enums And Match

Aurora now supports enum declarations with unit variants, single-payload variants, and exhaustive `match`.

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

## Matching Exhaustively

Aurora's current `match` support requires explicit coverage of every enum variant:

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

## Payload Bindings

When a case matches a payload variant, the payload name becomes available inside that arm:

```python
case ParseResult.Success(value):
    return value
```

## Current Limits

The bootstrap compiler currently supports:

- non-generic and generic enums
- zero-payload and single-payload variants
- statement-form `match`
- variant patterns of the form `Enum.Variant` and `Enum.Variant(name)`

It does not yet support:

- borrowed `match`
- nested patterns
- wildcard patterns
- expression-form `match`
- keyword arguments for variant payload construction
- multi-payload variants

Built-in generic enums such as `Result[T, E]`, `Option[T]`, and `SendError[T]` are covered in the next chapter.

See [examples/enums/result_match.au](../examples/enums/result_match.au).
