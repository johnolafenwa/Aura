# Expressions

Expressions produce values. Aurora currently supports the forms below.

## Names And Literals

```python
count
42
3.14
"text"
f"count={count}"
true
None
10ms
```

## Operators

| Operator | Notes |
| --- | --- |
| `+`, `-`, `*`, `/`, `%` | Numeric arithmetic; operator traits may overload supported operators. |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | Equality and ordering where supported. |
| `and`, `or`, `not` | Boolean logic. |
| unary `-` | Numeric negation. |
| `expr as Type` | Explicit numeric cast. |

Division by zero and checked integer overflow are runtime errors.

## Calls

```python
print("hello")
range(1, 4)
process.run(["echo", "hi"], stdout=process.pipe(), group=true)
```

Calls support positional arguments first, followed by named arguments.

## Members And Indexing

```python
point.x
point.distance()
values[0]
counts["ready"]
```

Copy-typed vector element reads can use indexing directly. Non-copy vector element reads should use `get(index)` so the clone is explicit.

## Collection Literals

```python
values = [1, 2, 3]
counts = {"ready": 2, "done": 1}
seen = {1, 2, 3}
```

Empty literals need expected types:

```python
values: Vec[int32] = []
counts: Map[String, int32] = {}
seen: Set[int32] = {}
```

## Match Expressions

```python
label = match code:
    case 0:
        "ok"
    case _:
        "other"
```

Expression-form `match` is valid in return, binding, and argument positions.

## try

```python
def parse_value(text: String) -> Result[int32, String]:
    value = try parse_int32(text)
    return Result.Ok(value)
```

`try` requires the containing function to return `Result`.

## Enum Construction

```python
Result[int32, String].Ok(7)
Option.Some("name")
Status.Ready(count=3)
```

Builtin enum variants may be matched without qualification when the scrutinee type is known. Constructors should use the enum name:

```python
result: Result[int32, String] = Result.Ok(7)
maybe: Option[String] = Option.None
```
