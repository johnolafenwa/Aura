# Bindings And Types

In Aurora, every value has a type known at compile time. Bindings are introduced with assignment -- no `let` keyword is needed.

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
mut names: Vec[String] = []
mut counts: Map[String, int32] = {}
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

Aurora uses `None` as both the unit type and the sole unit value:

```python
status: None = None
```

Functions that omit a return type annotation implicitly return `None`. You will see this throughout the tutorials.

## Builtin Scalar Types

Aurora has a rich set of numeric types. If you are not sure which to use, start with `int` for integers and `float64` for decimals:

| Type | Description | When to use |
|------|-------------|-------------|
| `int` | Alias for `int64` | Default integer spelling |
| `int32` | 32-bit signed integer | Fixed-width APIs and 32-bit range/layout contracts |
| `int64` | 64-bit signed integer | Same type as `int`; large counts and timestamps |
| `float64` | 64-bit floating point | Default for decimal math |
| `float32` | 32-bit floating point | When memory or precision constraints require it |
| `bool` | `true` or `false` | Conditions and flags |
| `String` | Owned text | Any text data |
| `Duration` | Time span | Concurrency timeouts (`5ms`, `1s`, `2m`) |
| `None` | Unit type | Functions with no meaningful return |

The full set of integer types covers `int8` through `int128`, `uint8` through `uint128`, plus `intsize` and `uintsize` for platform-sized integers. `int` is not an additional width: it is exactly `int64`. Use other explicit widths when you need control over memory layout, value ranges, or a fixed-width API contract.

Integer literals default to `int64`. Floating-point literals default to `float64`, but both kinds of literal adopt a compatible expected numeric type from an annotation, parameter, return type, or field:

```python
count: int32 = 12
ratio: float32 = 3.25
```

The default-type change does not alter APIs that explicitly use `int32`. For example, `range(...)`, Vec indexes, collection lengths, queue capacities, and a numeric `main()` exit status remain `int32`; literals passed to them adopt that expected type.

That context applies to the literal expression itself, not to a binding created earlier. `values.get(0)` uses the required `int32` context, but `index = 0` creates an `int64` binding and cannot later be used as a Vec index. Write `index: int32 = 0` when the binding is meant for a fixed-width index API.

## Builtin Container Types

Aurora provides three owned collection types and several runtime types:

| Type | Description |
|------|-------------|
| `Vec[T]` | Ordered, growable list |
| `Map[K, V]` | Key-value map |
| `Set[T]` | Unordered collection of unique values |
| `Option[T]` | A value that may or may not be present |
| `Result[T, E]` | Success or failure |
| `Queue[T]` | Typed queue for concurrency |
| `Task[T]` | Handle to a spawned task |
| `TaskGroup` | Structured task scope |

`Option[T]` and `Result[T, E]` are covered in [10-results-and-options.md](10-results-and-options.md). Queues and tasks are covered in [13-concurrency.md](13-concurrency.md).

## `Vec[T]` And List Literals

Create a vector with a list literal:

```python
mut numbers = [1, 2, 3]
```

Or with the explicit empty constructor:

```python
values = Vec[int32]()
```

The element type must be consistent:

```python
mut ok = [1, 2, 3]
mut bad = [1, "two"]  # rejected: mixed types
```

Empty list literals need a type annotation:

```python
mut names: Vec[String] = []
```

Common vector operations:

```python
mut items = [10, 20, 30]
items.push(40)             # append an element
print(items.len())         # 4
print(items[0])            # 10 -- indexed access
print(items.contains(20))  # true
popped = items.pop()       # removes and returns the last element
```

Negative Vec indexes count from the end. The same normalization applies to
direct reads and writes and to `get`, `set`, `remove`, `swap`, and `insert`:

```python
print(items[-1])                 # final element
match items.get(-2):
    case Option.Some(value):
        print(value)
    case Option.None:
        pass

items[-1] = 50
items.insert(-1, 45)             # inserts before the final element
items.insert(items.len(), 60)    # appends
```

Normalization is `len + index`, performed once. `get` returns `None` if the
result is still out of range; direct access and the mutating methods raise a
runtime error. Unlike Python, `insert` does not clamp an extremely negative
index to zero, because silently inserting at the wrong position hides bugs.

The full method surface includes `len`, `is_empty`, `clone`, `push`, `pop`, `get`, `insert`, `set`, `remove`, `swap`, `contains`, `extend`, `clear`, and `reverse`.

Because `len()` returns `int32`, you can use it directly with `range(...)`:

```python
for index in range(items.len()):
    print(items[index])
```

Indexed reads work as ordinary expressions, so chains like `keys[idx].clone()` are supported.
For non-copy element types like `String` or user-defined classes, indexed reads require `get(index)` instead of `items[index]` so the cloned read stays explicit:

```python
names = ["Ada", "Grace"]
match names.get(0):
    case Option.Some(value):
        print(value)
    case Option.None:
        pass
```

See [examples/collections/vec_basics.au](../examples/collections/vec_basics.au), [examples/collections/vec_iteration.au](../examples/collections/vec_iteration.au), and [examples/collections/vec_polish.au](../examples/collections/vec_polish.au).

For integer types, the runtime enforces the annotated width. A binding like `value: int8 = 127` is valid, but exceeding that range at runtime produces an error instead of silently widening the value.

## `Map[K, V]` And Map Literals

Create a map with a literal:

```python
mut counts = {"aurora": 1, "codex": 2}
```

Or with the explicit empty constructor:

```python
counts = Map[String, int32]()
```

Empty map literals need a type annotation:

```python
mut counts: Map[String, int32] = {}
```

Maps support indexed reads and writes:

```python
counts["aurora"] = 5
print(counts["aurora"])
```

Map lookups work inside larger expressions including f-strings:

```python
print(f"value: {counts['aurora']}")
```

`items()` and `entries()` both return `Vec[MapEntry[K, V]]`, where each entry exposes `.key` and `.value`:

```python
entries = counts.items()
match entries.get(0):
    case Option.Some(entry):
        print(entry.key)
        print(entry.value)
    case Option.None:
        pass
```

The full method surface includes `len`, `is_empty`, `clone`, `get`, `set`, `remove`, `contains_key`, `keys`, `values`, `items`, `entries`, `clear`, and `extend`.

See [examples/collections/map_basics.au](../examples/collections/map_basics.au).

## `Set[T]` And Set Literals

Create a set with curly braces when the entries are values rather than `key: value` pairs:

```python
mut seen = {1, 2, 2, 3}       # duplicates are removed
print(seen.len())              # 3
```

Or with the explicit empty constructor:

```python
names = Set[String]()
```

Empty set literals need a type annotation:

```python
mut names: Set[String] = {}
```

The full method surface includes `len`, `is_empty`, `clone`, `contains`, `insert`, and `remove`.

Sets deduplicate values and can be iterated by value or through a shared borrow.

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## Literal Defaults

Summary of literal type rules:

- integer literals default to `int64` (`int` is an alias for `int64`)
- floating-point literals default to `float64`
- duration literals like `5ms`, `1s`, and `2m` have type `Duration`
- negative literals are supported: `-5`, `-3.5`

```python
offset: int32 = -5
temperature: float64 = -3.5
short_wait: Duration = 5ms
```
