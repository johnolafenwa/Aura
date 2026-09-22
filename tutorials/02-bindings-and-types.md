# Bindings And Types

Every Aura value has a type known at compile time. You create a binding with
plain assignment. There is no `let` keyword.

## Inferred Bindings

The compiler infers the type from the right-hand side:

```aura check-pass
a = 56
b = 100
total = a + b
```

`a`, `b`, and `total` are all `int64`, because integer literals default to
`int64`. The shorter spelling `int` is an alias for `int64`.

See [examples/basics/top_level_script.au](../examples/basics/top_level_script.au).

## Annotated Bindings

Write the type yourself when you want to be explicit or the compiler needs
help:

```aura check-pass
a: int32 = 6
b: int32 = 10
c: int32 = a + b
```

An annotation is required when the compiler cannot infer the type. Empty
collections are the common case:

```aura check-pass
mut names: list[str] = []
mut counts: dict[str, int32] = {}
```

See [examples/basics/main_function.au](../examples/basics/main_function.au).

## Mutable Bindings

Bindings are immutable by default. Use `mut` when you need to reassign:

```aura check-pass
mut counter: int32 = 1
counter = counter + 1
counter += 3
```

Without `mut`, the compiler rejects the reassignment. Immutable-by-default
makes it easy to see which values change.

Assigning to an existing name updates that binding. The compiler does not
create a new shadowed binding in the same scope.

See [examples/basics/mutable_bindings.au](../examples/basics/mutable_bindings.au).

## `None` Is The Unit Type And Value

`None` is both the unit type and its only value:

```aura check-pass
status: None = None
```

A function that omits its return type returns `None`. You will see this
throughout the tutorials.

## Builtin Scalar Types

If you are not sure which numeric type to use, start with `int` for integers
and `float64` for decimals:

| Type | Description | When to use |
|------|-------------|-------------|
| `int` | Alias for `int64` | Default integer spelling |
| `int32` | 32-bit signed integer | Fixed-width APIs and 32-bit range/layout contracts |
| `int64` | 64-bit signed integer | Same type as `int`. Large counts and timestamps |
| `float64` | 64-bit floating point | Default for decimal math |
| `float32` | 32-bit floating point | When memory or precision constraints require it |
| `bool` | `true` or `false` | Conditions and flags |
| `str` | Owned text | Any text data |
| `Duration` | Signed nanosecond time span | Computed backoff and concurrency timeouts (`5ms`, `1s`, `2m`) |
| `None` | Unit type | Functions with no meaningful return |

The full set of integer types is `int8` through `int128`, `uint8` through
`uint128`, and the platform-sized `intsize` and `uintsize`. `int` is not an
extra width. It is exactly `int64`. Use another explicit width when you need
control over memory layout, value ranges, or a fixed-width API contract.

The runtime enforces the annotated integer width. `value: int8 = 127` is
valid. Going past that range at runtime is an error, and the value keeps its
declared type.

### Numeric Literals

Integer literals default to `int64` and floating-point literals default to
`float64`. Both adopt a compatible expected numeric type from an annotation,
parameter, return type, or field. An integer literal may adopt `float32` or
`float64` only when that type represents its value exactly:

```aura check-pass
count: int32 = 12
ratio: float32 = 3.25
whole_ratio: float64 = 2
```

This rule applies only to literals. It never converts an integer value that is
already bound. If an integer literal is not exact in the expected floating
type, the compiler asks for an explicit floating spelling or `.to_float()`.
That keeps any rounding visible in the source.

### Positions And Lengths

Position APIs use `int64`. Positions include ranges, list indexes, slice
endpoints, enumeration positions, and Array coordinates. Length members match
that domain: `str.len()`, `str.byte_len()`, `list.len()`, `dict.len()`, and
`set.len()` all return `int64`.

Some APIs use exact `int32` instead. These include queue capacities and a
numeric `main()` exit status.

A literal such as the `0` in `values.get(0)` or `index = 0` is an `int64`, so
the binding can go straight into an index operation. Bindings of type `int8`,
`int16`, `int32`, `uint8`, `uint16`, and `uint32` widen losslessly only where a
position is expected. Ordinary assignments and function arguments still
require exact types.

## Builtin Container Types

Aura has three owned collection types and several runtime types:

| Type | Description |
|------|-------------|
| `list[T]` | Ordered, growable list |
| `dict[K, V]` | Key-value dictionary |
| `set[T]` | Unordered collection of unique values |
| `Array[T]` | Fixed-shape contiguous numeric array. `T` is `int32`, `int64`, `float32`, or `float64` |
| `T \| None` | An optional value: a union whose last member is `None` |
| `Lookup[T]` | A collection lookup outcome: `Found(value)` or `Missing` |
| `Result[T, E]` | Success or failure |
| `Queue[T]` | Typed queue for concurrency |
| `Task[T]` | Handle to a spawned task |
| `TaskGroup` | Structured task scope |

[10-results-and-options.md](10-results-and-options.md) covers `T | None`,
`Lookup[T]`, and `Result[T, E]`. [13-concurrency.md](13-concurrency.md) covers
queues and tasks.

`Array[T]` is an owned, non-Copy value. Its shape is fixed and has rank one or
more. Build one with an Array constructor:

```aura check-pass
source: list[float64] = [1.0, 2.0, 3.0, 4.0]
matrix = Array[float64].from_list(source, [2, 2])
zeros = Array[int32].zeros([3, 4])
filled = Array[float32].full([2, 2], 0.5)
```

`from_list` copies the scalar elements, so `source` stays usable. Assigning an
Array transfers ownership. `.clone()` returns an independent Array. All four
Array specializations satisfy `Transfer`. See
[examples/numbers/numeric_arrays.au](../examples/numbers/numeric_arrays.au)
and the [Numeric Arrays Manual](../docs/manual/numeric-arrays.md).

## `list[T]` And List Literals

Create a list with a list literal:

```aura check-pass
mut numbers = [1, 2, 3]
```

Or with the explicit empty constructor:

```aura check-pass
values = list[int32]()
```

Every element must have the same type. A mixed list fails with `AU2002`:

```aura check-fail:AU2002
mut ok = [1, 2, 3]
mut bad = [1, "two"]  # rejected: mixed types
```

An empty list literal needs a type annotation:

```aura check-pass
mut names: list[str] = []
```

Common list operations:

```aura check-pass
def main():
    mut items = [10, 20, 30]
    items.append(40)       # append an element
    print(items.len())     # 4
    print(items[0])        # 10 -- indexed access
    print(20 in items)     # true
    popped = items.pop()   # removes and returns the last element
    print(popped)          # 40
```

The list methods are `len`, `is_empty`, `copy`, `append`, `pop`, `get`,
`insert`, `set`, `remove`, `index`, `count`, `swap`, `extend`, `clear`,
`reverse`, `sort`, `map`, `filter`, `reserve`, and `with_capacity`.

### Negative Indexes

A negative index counts from the end. This works for direct reads and writes
and for `get`, `set`, `pop`, and `swap`:

```aura fragment
print(items[-1])                 # final element
match items.get(-2):
    case Lookup.Found(value):
        print(value)
    case Lookup.Missing:
        pass

items[-1] = 50
items.insert(-1, 45)             # inserts before the final element
end_index: int64 = items.len()
items.insert(end_index, 60)      # appends
```

Aura normalizes a negative index once, to `len + index`. If the result is
still out of range:

- `get` returns `Lookup.Missing`
- direct access, `pop`, `set`, and `swap` raise a runtime error
- `insert` clamps the position to the range from zero through the current length

### Slices

A slice returns a fresh owned list:

```aura check-pass
values = [10, 20, 30, 40]
middle = values[1:3]  # [20, 30]
prefix = values[:2]   # [10, 20]
suffix = values[-2:]  # [30, 40]
copy = values[:]      # an independent list
```

Slices follow strict bounds rules:

- Every written endpoint is an `int64` position, and a negative endpoint
  normalizes once.
- Both effective bounds must be in `0..=len`.
- A start greater than the end is an `AU4003` runtime error.

Aura does not copy Python's clamping, or its rule that a reversed range is
empty. Slicing copies Copy elements and clones clone-safe non-Copy elements.
A clone-safe type is one whose values can be cloned. A slice is never a view.

### Equality

Operations that compare elements need an element type that defines equality.
These are `remove`, `index`, `count`, `in`, and `not in`. Closures,
`random.Rng`, opaque FFI handles, and values containing them have no equality.
The compiler rejects these operations on them with `AU2008`.

### `map`, `filter`, And `sort`

The callable-powered algorithms take named function values:

```aura check-pass
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

- `map` and `filter` read the list eagerly through shared access. They return
  fresh owned lists and leave `values` in place.
- `filter` clones the elements it keeps, so the element type must be
  clone-safe.
- Natural and keyed `sort` calls mutate the list in place, and the sort is
  stable.
- The `key` callback runs once per element, left to right, before any
  mutation. If the key callback traps, the list is unchanged.
- A callback takes its element as a bare shared parameter, as shown above. It
  cannot take the element as `mut` or `own`.

### Lengths

`list.len()`, `range(...)`, and list indexes all use `int64` positions:

```aura fragment
for index in range(items.len()):
    print(items[index])
```

The free `len(value)` builtin calls the same member and also returns `int64`:

```aura fragment
assert len(items) == items.len()
assert len("A🎉") == "A🎉".len()
```

For `str`, `len()` counts Unicode scalar values and `byte_len()` counts UTF-8
bytes. `"A🎉".len()` is `2` and `"A🎉".byte_len()` is `5`.

### Non-Copy Elements

Indexed reads work directly when the element type is copy. For a non-copy
element, choose how to access it:

- `view item = items[index]` reads the element in place.
- `view mut item = items[index]` updates it in place through a mutable source.
- `get(index)` returns an owned clone when the element is clone-safe.
- `pop(index)` transfers the stored element out.

Writing `.clone()` directly after `items[index]` does not repair the read. The
illegal move happens before the method call. A value containing `random.Rng`
cannot be cloned, so taking ownership of it requires a transfer such as
`pop(index)`.

For a clone-safe value such as `str`, handle the owned `get` result
explicitly:

```aura check-pass
names = ["Ada", "Grace"]
match names.get(0):
    case Lookup.Found(value):
        print(value)
    case Lookup.Missing:
        pass
```

See [examples/collections/list_basics.au](../examples/collections/list_basics.au),
[examples/collections/list_iteration.au](../examples/collections/list_iteration.au),
[examples/collections/list_polish.au](../examples/collections/list_polish.au),
[examples/collections/slices.au](../examples/collections/slices.au),
and
[examples/collections/list_algorithms.au](../examples/collections/list_algorithms.au).

## `dict[K, V]` And Dictionary Literals

Create a dictionary with a literal:

```aura check-pass
mut counts = {"aura": 1, "codex": 2}
```

Or with the explicit empty constructor:

```aura check-pass
counts = dict[str, int32]()
```

An empty dictionary literal needs a type annotation:

```aura check-pass
mut counts: dict[str, int32] = {}
```

Indexed writes work for every value type. Indexed reads work when the value
type is copy:

```aura fragment
counts["aura"] = 5
print(counts["aura"])
```

A lookup works inside larger expressions, including f-strings:

```aura fragment
print(f"value: {counts['aura']}")
```

Aura never clones behind your back. For a non-copy value type, the compiler
rejects reading `dictionary[key]` into an owned value. Choose one of these
instead:

- `view entry = dictionary[key]` reads the entry in place.
- `view mut entry = dictionary[key]` updates it in place through a mutable
  dictionary.
- `get(key)` returns an explicit clone when the value type is clone-safe.
- `remove(key)` transfers the stored value out.

A value carrying `random.Rng` state cannot be cloned. Use `remove(key)` to take
ownership of it. An entry view can still access it in place.

`items()` returns `list[(K, V)]` in insertion order:

```aura fragment
entries = counts.items()
match entries.get(0):
    case Lookup.Found((key, value)):
        print(key)
        print(value)
    case Lookup.Missing:
        pass
```

The dictionary methods are `len`, `is_empty`, `copy`, `get`, `remove`, `keys`,
`values`, `items`, `clear`, `update`, `reserve`, and `with_capacity`. Use
indexed assignment to store and `in` to test membership.

See [examples/collections/dict_basics.au](../examples/collections/dict_basics.au).

## `set[T]` And Set Literals

A set literal holds bare values inside curly braces. A dictionary literal uses
`key: value` pairs instead:

```aura check-pass
mut seen = {1, 2, 2, 3}       # duplicates are removed
print(seen.len())              # 3
```

Or use the explicit empty constructor. An empty set always uses this typed
constructor:

```aura check-pass
names = set[str]()
```

The set methods are `len`, `is_empty`, `copy`, `add`, `remove`, `discard`,
`clear`, `reserve`, and `with_capacity`. Use `in` to test membership.

Bare iteration over a set is shared. `for value in own set:` consumes the set.

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## Owned Comprehension Results

List, set, and dictionary comprehensions build fresh owned collections:

```aura check-pass
values = [1, 2, 3, 4]
squares = [value * value for value in values]
even = {value for value in values if value % 2 == 0}
labels = {value: str(value) for value in values}
```

Each clause follows the same rules as a bare `for value in values:` loop. The
loop variable over a list or set is shared. To store a non-copy loop variable
in the new collection, clone it explicitly:

```aura check-pass
names = ["Ada", "Grace"]
names_copy = [name.clone() for name in names]
```

Aura does not clone silently. Queue is the one exception to shared access. A
bare Queue clause receives each item already owned, so the item can move
straight into the result. The result collection is always owned and built
eagerly.

See
[examples/collections/comprehensions.au](../examples/collections/comprehensions.au).

## Literal Defaults

A summary of the literal type rules:

- Integer literals default to `int64`. `int` is an alias for `int64`.
- An integer literal can adopt an expected floating type only when that type
  represents it exactly.
- Floating-point literals default to `float64`.
- Duration literals like `5ms`, `1s`, and `2m` have type `Duration`.
- Negative numeric literals such as `-5` and `-3.5` work. Duration literals
  cannot be negative. Use a constructor such as `Duration.ms(-5)` for a
  negative Duration value.

```aura check-pass
offset: int32 = -5
temperature: float64 = -3.5
short_wait: Duration = 5ms
```
