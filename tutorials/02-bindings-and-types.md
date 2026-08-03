# Bindings And Types

In Aura, every value has a type known at compile time. Bindings are introduced with assignment -- no `let` keyword is needed.

## Inferred Bindings

The compiler infers the type from the right-hand side:

```python
a = 56
b = 100
total = a + b
```

Here `a`, `b`, and `total` are all `int64` because integer literals default to `int64`. The shorter type spelling `int` is an alias for `int64`.

See [examples/basics/top_level_script.au](../examples/basics/top_level_script.au).

## Annotated Bindings

You can write the type explicitly when you want to be clear or when the compiler needs help:

```python
a: int32 = 6
b: int32 = 10
c: int32 = a + b
```

Type annotations are required when the compiler cannot infer the type, for example with empty collections:

```python
mut names: list[str] = []
mut counts: dict[str, int32] = {}
```

See [examples/basics/main_function.au](../examples/basics/main_function.au).

## Mutable Bindings

Bindings are immutable by default. Use `mut` when you need to reassign:

```python
mut counter: int32 = 1
counter = counter + 1
counter += 3
```

If you forget `mut` and try to reassign, the compiler will reject the code. This is intentional -- immutable by default makes it easy to see which values change.

See [examples/basics/mutable_bindings.au](../examples/basics/mutable_bindings.au).

Reusing an existing name updates that binding. The current compiler does not create a new shadowed binding in the same scope.

## `None` Is The Unit Type And Value

Aura uses `None` as both the unit type and the sole unit value:

```python
status: None = None
```

Functions that omit a return type annotation implicitly return `None`. You will see this throughout the tutorials.

## Builtin Scalar Types

Aura has a rich set of numeric types. If you are not sure which to use, start with `int` for integers and `float64` for decimals:

| Type | Description | When to use |
|------|-------------|-------------|
| `int` | Alias for `int64` | Default integer spelling |
| `int32` | 32-bit signed integer | Fixed-width APIs and 32-bit range/layout contracts |
| `int64` | 64-bit signed integer | Same type as `int`; large counts and timestamps |
| `float64` | 64-bit floating point | Default for decimal math |
| `float32` | 32-bit floating point | When memory or precision constraints require it |
| `bool` | `true` or `false` | Conditions and flags |
| `str` | Owned text | Any text data |
| `Duration` | Signed nanosecond time span | Computed backoff and concurrency timeouts (`5ms`, `1s`, `2m`) |
| `None` | Unit type | Functions with no meaningful return |

The full set of integer types covers `int8` through `int128`, `uint8` through `uint128`, plus `intsize` and `uintsize` for platform-sized integers. `int` is not an additional width: it is exactly `int64`. Use other explicit widths when you need control over memory layout, value ranges, or a fixed-width API contract.

Integer literals default to `int64`. Floating-point literals default to `float64`, but both kinds of literal adopt a compatible expected numeric type from an annotation, parameter, return type, or field. An integer literal may adopt `float32` or `float64` only when its integer value is exactly representable there:

```python
count: int32 = 12
ratio: float32 = 3.25
whole_ratio: float64 = 2
```

This float-context rule applies only to literals. It never converts an already-bound integer value. If an integer literal is not exact in the expected floating type, the compiler asks you to use an explicit floating spelling or `.to_float()` so that rounding is visible in the source.

APIs that explicitly use `int32`, including queue capacities and a numeric
`main()` exit status, remain exact `int32` contracts. Position APIs use
`int64`: this includes ranges, list indexes, slice endpoints, enumeration
positions, and Array coordinates. Length members match that position domain:
`str.len()`, `str.byte_len()`,
`list.len()`, `dict.len()`, and `set.len()` all return `int64`.

`values.get(0)` and `index = 0` both use `int64`, so the binding can flow
directly into an index operation. Fixed-width `int8`, `int16`, `int32`,
`uint8`, `uint16`, and `uint32` bindings widen losslessly only at position
sites. Ordinary assignments and function arguments still require exact types.

## Builtin Container Types

Aura provides three owned collection types and several runtime types:

| Type | Description |
|------|-------------|
| `list[T]` | Ordered, growable list |
| `dict[K, V]` | Key-value dictionary |
| `set[T]` | Unordered collection of unique values |
| `Array[T]` | Fixed-shape contiguous numeric array; `T` is `int32`, `int64`, `float32`, or `float64` |
| `Option[T]` | A value that may or may not be present |
| `Result[T, E]` | Success or failure |
| `Queue[T]` | Typed queue for concurrency |
| `Task[T]` | Handle to a spawned task |
| `TaskGroup` | Structured task scope |

`Option[T]` and `Result[T, E]` are covered in [10-results-and-options.md](10-results-and-options.md). Queues and tasks are covered in [13-concurrency.md](13-concurrency.md).

`Array[T]` is an owned non-Copy value with a fixed rank-at-least-one shape.
Construct it explicitly with an Array constructor:

```python
source: list[float64] = [1.0, 2.0, 3.0, 4.0]
matrix = Array[float64].from_list(source, [2, 2])
zeros = Array[int32].zeros([3, 4])
filled = Array[float32].full([2, 2], 0.5)
```

`from_list` copies the scalar elements, so `source` remains usable. Assignment
of an Array transfers ownership, while `.clone()` returns an independent
Array. All four maintained Array specializations satisfy `Transfer`. See
[examples/numbers/numeric_arrays.au](../examples/numbers/numeric_arrays.au)
and the [Numeric Arrays Manual](../docs/manual/numeric-arrays.md).

## `list[T]` And List Literals

Create a list with a list literal:

```python
mut numbers = [1, 2, 3]
```

Or with the explicit empty constructor:

```python
values = list[int32]()
```

The element type must be consistent:

```python
mut ok = [1, 2, 3]
mut bad = [1, "two"]  # rejected: mixed types
```

Empty list literals need a type annotation:

```python
mut names: list[str] = []
```

Common list operations:

```python
mut items = [10, 20, 30]
items.append(40)             # append an element
print(items.len())         # 4
print(items[0])            # 10 -- indexed access
print(20 in items)         # true
popped = items.pop()       # removes and returns the last element
```

Negative list indexes count from the end. The same normalization applies to
direct reads and writes and to `get`, `set`, `pop`, and `swap`:

```python
print(items[-1])                 # final element
match items.get(-2):
    case Option.Some(value):
        print(value)
    case Option.None:
        pass

items[-1] = 50
items.insert(-1, 45)             # inserts before the final element
end_index: int64 = items.len()
items.insert(end_index, 60)      # appends
```

Normalization is `len + index`, performed once. `get` returns `None` if the
result is still out of range; direct access, `pop`, `set`, and `swap` raise a
runtime error. `insert` clamps positions to the range from zero through the
current length.

List slicing uses the same loud boundary philosophy and returns a fresh owned
list:

```python
values = [10, 20, 30, 40]
middle = values[1:3]  # [20, 30]
prefix = values[:2]   # [10, 20]
suffix = values[-2:]  # [30, 40]
copy = values[:]      # an independent list
```

Every written endpoint uses the `int64` position domain, negatives normalize once, and both
effective bounds must be in `0..=len`. A start greater than end is also an
`AU4003` runtime error. Aura does not copy Python's clamping or
reversed-range-as-empty behavior. Slicing copies Copy elements and clones
clone-safe non-Copy elements; it never creates a view.

The method surface includes `len`, `is_empty`, `copy`, `append`, `pop`, `get`,
`insert`, `set`, `remove`, `index`, `count`, `swap`, `extend`, `clear`,
`reverse`, `sort`, `map`, `filter`, `reserve`, and `with_capacity`.

The four callable-powered algorithms use named function values:

```python
def doubled(value: int32) -> int32:
    return value * 2

def is_even(value: int32) -> bool:
    return value % 2 == 0

values: list[int32] = [3, 1, 2, 4]
mapped = values.map(doubled)
filtered = values.filter(is_even)

mut ordered = values.copy()
ordered.sort()
```

`map` and `filter` are eager shared reads that return fresh owned lists and
retain `values`. `filter` clones accepted elements, so the element type must be
clone-safe. Natural and keyed `sort` calls are stable in-place mutations. The
`key` callback runs once per element from left to right before mutation; a key
trap leaves the list unchanged. Algorithm callbacks take their element with
the exact bare/shared capability shown above, not `mut` or `own`.

`list.len()`, `range(...)`, and list indexes share the `int64` position domain:

```python
for index in range(items.len()):
    print(items[index])
```

The free `len(value)` builtin delegates to the same member and has the same
`int64` result:

```python
assert len(items) == items.len()
assert len("A🎉") == "A🎉".len()
```

For `str`, `len()` counts Unicode scalar values and `byte_len()` counts the
UTF-8 encoding bytes. Both counts are `int64`, so `"A🎉".len()` is `2` while
`"A🎉".byte_len()` is `5`.

Indexed reads work as ordinary expressions, so chains like `keys[idx].clone()` are supported.
For clone-safe non-copy element types like `str` or ordinary user-defined
classes, use `get(index)` for an explicit cloned read. Direct `items[index]`
access is rejected. A value containing `random.Rng` must be transferred with
`pop(index)` because it cannot be cloned, and the rejection names that
reason directly:

```python
names = ["Ada", "Grace"]
match names.get(0):
    case Option.Some(value):
        print(value)
    case Option.None:
        pass
```

See [examples/collections/list_basics.au](../examples/collections/list_basics.au),
[examples/collections/list_iteration.au](../examples/collections/list_iteration.au),
[examples/collections/list_polish.au](../examples/collections/list_polish.au),
[examples/collections/slices.au](../examples/collections/slices.au),
and
[examples/collections/list_algorithms.au](../examples/collections/list_algorithms.au).

For integer types, the runtime enforces the annotated width. A binding like
`value: int8 = 127` is valid, but exceeding that range at runtime produces an
error and preserves the declared type.

## `dict[K, V]` And Dictionary Literals

Create a dictionary with a literal:

```python
mut counts = {"aura": 1, "codex": 2}
```

Or with the explicit empty constructor:

```python
counts = dict[str, int32]()
```

Empty dictionary literals need a type annotation:

```python
mut counts: dict[str, int32] = {}
```

Dictionaries support indexed reads when the value type is copy, and indexed writes for
all value types:

```python
counts["aura"] = 5
print(counts["aura"])
```

Dictionary lookups work inside larger expressions including f-strings:

```python
print(f"value: {counts['aura']}")
```

For a non-copy value type, direct `dictionary[key]` is rejected; Aura never performs a
hidden clone. When the value type is clone-safe, `get(key)` gives an explicit
cloned optional read and `remove(key)` transfers the stored value out. When the
value type carries `random.Rng` state, only `remove(key)` works, and the
rejection explains that `get(key)` would also be rejected.

`items()` returns `list[(K, V)]` in insertion order:

```python
entries = counts.items()
match entries.get(0):
    case Option.Some((key, value)):
        print(key)
        print(value)
    case Option.None:
        pass
```

The method surface includes `len`, `is_empty`, `copy`, `get`, `remove`, `keys`,
`values`, `items`, `clear`, `update`, `reserve`, and `with_capacity`. Use indexed
assignment for storage and `in` for membership.

See [examples/collections/dict_basics.au](../examples/collections/dict_basics.au).

## `set[T]` And Set Literals

Create a set with value-only entries inside curly braces. Dictionary literals use
`key: value` pairs:

```python
mut seen = {1, 2, 2, 3}       # duplicates are removed
print(seen.len())              # 3
```

Or with the explicit empty constructor:

```python
names = set[str]()
```

Empty sets use the typed constructor shown above.

The method surface includes `len`, `is_empty`, `copy`, `add`, `remove`,
`discard`, `clear`, `reserve`, and `with_capacity`. Use `in` for membership.

Sets deduplicate values. Bare iteration is shared; `for value in own set:`
consumes the set.

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## Owned Comprehension Results

List, set, and dictionary comprehensions build fresh owned collections:

```python
values = [1, 2, 3, 4]
squares = [value * value for value in values]
even = {value for value in values if value % 2 == 0}
labels = {value: str(value) for value in values}
```

Each clause uses the same bare-loop rules as `for value in values:`. A list or
Set target is shared, so storing a non-copy target in the new collection needs
an explicit clone:

```python
names = ["Ada", "Grace"]
names_copy = [name.clone() for name in names]
```

Aura does not silently clone. Queue is the existing exception: a bare Queue
clause receives each item already owned, so that item may move directly into
the result. The result collection is always owned and eager.

See
[examples/collections/comprehensions.au](../examples/collections/comprehensions.au).

## Literal Defaults

Summary of literal type rules:

- integer literals default to `int64` (`int` is an alias for `int64`)
- integer literals can adopt an expected floating type only when exactly representable
- floating-point literals default to `float64`
- duration literals like `5ms`, `1s`, and `2m` have type `Duration`
- negative numeric literals are supported: `-5`, `-3.5`; Duration literals
  remain non-negative, so use a constructor such as `Duration.ms(-5)` for a
  negative Duration value

```python
offset: int32 = -5
temperature: float64 = -3.5
short_wait: Duration = 5ms
```
