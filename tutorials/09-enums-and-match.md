# Enums And Match

An enum is a type whose value is one of several named variants. `match` picks an arm by variant, and the compiler checks that the arms cover every case. This chapter also shows how `match` and `is None` work on union types such as `int64 | str | None`.

## Declaring An Enum

```aura check-pass
enum TrafficLight:
    Red
    Yellow
    Green
```

Each variant lives in the enum's namespace: `TrafficLight.Red`, `TrafficLight.Yellow`, and `TrafficLight.Green`.

## Variants With Payloads

A variant can carry a single value, called its payload:

```aura check-pass
enum ParseResult:
    Success(int32)
    Failure(str)

ok = ParseResult.Success(42)
bad = ParseResult.Failure("invalid input")
```

A payload position is owned. `Failure(str)` acts like `Failure(own str)`. Builtin enums work the same way, as in `Lookup.Found(own T)` and `Result.Err(own E)`.

## Generic Enums

An enum can take type parameters:

```aura check-pass
enum Wrapper[T]:
    Item(T)
    Empty
```

When the compiler cannot infer the type arguments, write them explicitly:

```aura check-pass
wrapped = Result[int32, str].Ok(7)
```

See [examples/enums/explicit_type_args.au](../examples/enums/explicit_type_args.au).

## Exhaustive `match`

A `match` must handle every variant. If an arm is missing, the compiler reports an error. This function covers both variants of `ParseResult`:

```aura fragment
def value_or_zero(result: own ParseResult) -> int32:
    match result:
        case ParseResult.Success(value):
            return value
        case ParseResult.Failure(message):
            print(message)
            return 0
```

### Wildcard Arms

`case _:` matches every variant that no earlier arm matched:

```aura fragment
match light:
    case TrafficLight.Red:
        print("stop")
    case _:
        print("not red")
```

### Payload Bindings

When an arm matches a variant with a payload, the name in the pattern becomes a local binding for that payload:

```aura fragment
case ParseResult.Success(value):
    return value    # value is an int32 here
```

### Unqualified Variants

When the compiler already knows the scrutinee's type, you can leave out the enum name. The scrutinee is the value being matched.

```aura check-pass
result: Result[str, str] = Result.Ok("ok")

match result:
    case Ok(value):       # same as Result.Ok(value)
        print(value)
    case Err(message):    # same as Result.Err(message)
        print(message)
```

This is most useful with builtin enums such as `Result` and `Lookup`.

## Borrowed Matching

A bare `match` inspects the value without consuming it. Write `match own` when an arm must receive owned payloads. The difference matters for non-copy types, as [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) explains.

```aura check-pass
result: Result[str, str] = Result.Ok("ok")

match result:
    case Ok(value):
        print(value.clone())    # value is a borrowed str
    case Err(message):
        print(message)

# result is still valid here
```

Write `match mut` when an arm must modify the matched value:

```aura check-pass
mut result: Result[str, str] = Result.Ok("hello")
match mut result:
    case Ok(msg):
        pass    # msg is mut str
    case Err(e):
        pass
```

The scrutinee can be a field such as `holder.state`. Reassigning that field, `holder`, or an ancestor field makes the arm's payload bindings stale. Changing a separate sibling field is allowed. See [examples/enums/match_borrow_mut_fields.au](../examples/enums/match_borrow_mut_fields.au) and [examples/enums/match_borrow.au](../examples/enums/match_borrow.au).

## Literal Match Patterns

A pattern can be a literal `bool`, integer, or `str` value:

```aura check-pass
def describe_number(value: int32) -> str:
    match value:
        case 0:
            return "zero"
        case 1:
            return "one"
        case _:
            return "many"
```

A `bool` match is exhaustive when it covers both `true` and `false`:

```aura check-pass
def describe_flag(flag: bool) -> str:
    match flag:
        case true:
            return "yes"
        case false:
            return "no"
```

Integer and `str` matches always need a final wildcard arm, because their set of possible values is open-ended. See [examples/control_flow/match_literals.au](../examples/control_flow/match_literals.au).

`match` also supports:

- nested patterns
- expression-form `match`, which produces a value
- floating-point literal patterns
- keyword payload arguments
- variants with several payloads

```aura check-pass
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

A guard is an `if` condition on an arm. It runs after the pattern matches, and it must be exactly `bool`. An or-pattern lets one arm accept several alternatives, separated by `|`:

```aura fragment
match code:
    case 200 | 201 if code == 201:
        print("created")
    case 200 | 201:
        print("success")
    case _:
        print("other")
```

The rules:

- Alternatives are tried left to right.
- Every alternative must bind the same names, with the same types and capabilities.
- When a guard is false, matching continues with the next arm.
- Guarded arms do not count toward exhaustiveness. Keep an unguarded fallback when the remaining values are open-ended.

A lowercase name at the top level of a pattern binds the whole scrutinee. With a guard, the condition can use that name. Without a guard, the arm is the final catch-all:

```aura fragment
return match value:
    case whole if whole >= 0: whole
    case whole: 0 - whole
```

Guards interact with `match own` and `match mut`:

- In `match own`, a guard can inspect a non-copy candidate but cannot move it. The payload is extracted only after the guard is true.
- In `match mut`, changes a guard makes stay visible when the guard is false or propagates a failure.

See [examples/enums/match_guards_and_or_patterns.au](../examples/enums/match_guards_and_or_patterns.au).

Expression-form `match` works in more places than `return`. You can use it as a binding's value or as a call argument. An arm's value can itself be a nested block-form `match`:

```aura fragment
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

## Matching A Union Member

A union such as `int64 | str | None` holds a value of one of several existing types. Use a union when the alternatives are existing types. Select a member with `case Type as name`:

```aura check-pass
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

This prints `42` on two lines, because the integer arm changes the payload in place. An arm cannot replace the payload with a value of another member type. To change which member the union holds, assign the whole `value` after the arm ends.

The three forms behave as they do for enums:

- `match value` borrows a non-copy payload.
- `match mut value` lets the arm modify the payload.
- `match own value` consumes the payload after a guard commits.

Guards do not count toward coverage. Include an unguarded arm for every member, or a final `_`. Type arms also work inside an enum payload pattern.

Aura has no builtin `Option` type and no `T?` suffix. `T | None` is the only way to write an optional value. See [union_type_patterns.au](../examples/enums/union_type_patterns.au).

## Testing For `None`

To ask whether a `T | None` value is present, you do not need a `match`. `is None` and `is not None` narrow the tested place in the branch they select:

```aura check-pass
def describe(value: str | None):
    if value is None:
        print("missing")
        return
    print(value.len())

def main():
    describe(None)
    describe("aura")
```

After `if value is None: return`, and inside an `else` branch, `value` is a plain `str`. Narrowing works the same way for:

- `mut` locals, class fields, and views
- conditions combined with `not`, `and`, and `or`

These actions end the narrowing:

- assigning to the place
- matching it with `match mut`
- passing it to a call with `mut` access

Using the member after that reports `AU2014`. To fix it, test the value again. Only `is None` and `is not None` narrow. `== None` compares without narrowing.

See [union_narrowing.au](../examples/enums/union_narrowing.au).

The next chapter covers the builtin `Result[T, E]`, optional `T | None` unions, `Lookup[T]`, and `SendError[T]`. See [examples/enums/result_match.au](../examples/enums/result_match.au) and [examples/enums/wildcard_match.au](../examples/enums/wildcard_match.au).
