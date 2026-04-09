# Bindings And Types

Bindings are introduced with assignment. Aurora does not currently require a `let` keyword.

The bootstrap compiler supports both inferred and annotated bindings at top level and inside functions.

## Inferred Bindings

```python
a = 56
b = 100
total = a + b
```

This style is shown in [examples/basics/top_level_script.au](../examples/basics/top_level_script.au).

## Annotated Bindings

```python
a: int32 = 6
b: int32 = 10
c: int32 = a + b
```

This style is shown in [examples/basics/main_function.au](../examples/basics/main_function.au).

## Mutable Bindings

Bindings are immutable unless you declare them with `mut`.

```python
mut counter: int32 = 1
counter = counter + 1
counter += 3
```

See [examples/basics/mutable_bindings.au](../examples/basics/mutable_bindings.au).

Reusing an existing name updates that binding. The current compiler does not create a new shadowed binding in the same scope.

## `None` Is The Unit Type And Value

Aurora currently supports `None` as both:

- the unit type
- the sole unit value

```python
status: None = None
```

Functions that omit `-> None` still conceptually return this unit value.

## Builtin Types In The Implemented Subset

The bootstrap compiler currently recognizes these builtin scalar names:

- `bool`
- `int8`
- `int16`
- `int32`
- `int64`
- `int128`
- `intsize`
- `uint8`
- `uint16`
- `uint32`
- `uint64`
- `uint128`
- `uintsize`
- `float32`
- `float64`
- `String`
- `None`
- `Duration`

It also recognizes these builtin runtime or library-facing type names:

- `Range`
- `Vec[T]`
- `Map[K, V]`
- `Set[T]`
- `MapEntry[K, V]`
- `Channel[T]`
- `Task[T]`
- `TaskGroup`
- `Option[T]`
- `Result[T, E]`
- `SendError[T]`

## `Vec[T]` And List Literals

Aurora now includes a built-in owned vector type, `Vec[T]`.

You can introduce a vector with a list literal:

```python
mut numbers = [1, 2, 3]
```

or with the explicit empty constructor:

```python
values = Vec[int32]()
```

The element type must stay consistent inside a literal:

```python
mut ok = [1, 2, 3]
mut bad = [1, "two"]  # rejected
```

Empty list literals still need an expected `Vec[T]` type in the current bootstrap compiler:

```python
mut names: Vec[String] = []
```

Current vector methods include:

- `len() -> int32`
- `is_empty() -> bool`
- `clone() -> Vec[T]`
- `push(value: T) -> None`
- `pop() -> Option[T]`
- `get(index: int32) -> Option[T]`
- `insert(index: int32, value: T) -> bool`
- `set(index: int32, value: T) -> Option[T]`
- `remove(index: int32) -> Option[T]`
- `swap(first: int32, second: int32) -> bool`
- `contains(value: T) -> bool`
- `extend(other: Vec[T]) -> None`
- `clear() -> None`
- `reverse() -> None`

Because `len()` returns `int32`, you can use it directly with `range(...)`:

```python
for index in range(values.len()):
    print(values[index])
```

Indexed reads also stay ordinary expressions, so chains like `keys[idx].clone()` work as expected.

See [examples/collections/vec_basics.au](../examples/collections/vec_basics.au), [examples/collections/vec_iteration.au](../examples/collections/vec_iteration.au), and [examples/collections/vec_polish.au](../examples/collections/vec_polish.au).

For integer types, the current checker and runtimes both enforce the annotated width. A binding like `value: int8 = 127` is valid, but pushing that value out of range later will fail with a runtime diagnostic instead of silently widening it.

## `Map[K, V]` And Map Literals

Aurora also includes a built-in owned map type, `Map[K, V]`.

You can introduce a map with a literal:

```python
mut counts = {"aurora": 1, "codex": 2}
```

or with the explicit empty constructor:

```python
counts = Map[String, int32]()
```

Empty map literals still need an expected `Map[K, V]` type in the current bootstrap compiler:

```python
mut counts: Map[String, int32] = {}
```

Current map methods include:

- `len() -> int32`
- `is_empty() -> bool`
- `clone() -> Map[K, V]`
- `get(key: K) -> Option[V]`
- `set(key: K, value: V) -> Option[V]`
- `remove(key: K) -> Option[V]`
- `contains_key(key: K) -> bool`
- `keys() -> Vec[K]`
- `values() -> Vec[V]`
- `items() -> Vec[MapEntry[K, V]]`
- `entries() -> Vec[MapEntry[K, V]]`
- `clear() -> None`
- `extend(other: Map[K, V]) -> None`

Indexed reads and writes work directly on maps:

```python
counts["aurora"] = 5
print(counts["aurora"])
```

That also means map lookups can appear inside larger expressions such as f-strings:

```python
print(f"value: {counts["aurora"]}")
```

See [examples/collections/map_basics.au](../examples/collections/map_basics.au).

`items()` and `entries()` both return `Vec[MapEntry[K, V]]`, where each entry exposes `.key` and `.value`:

```python
entries = counts.items()
print(entries[0].key)
print(entries[0].value)
```

## `Set[T]` And Set Literals

Aurora also includes a built-in owned set type, `Set[T]`.

You can introduce a set with a literal:

```python
mut seen = Set{1, 2, 2, 3}
```

or with the explicit empty constructor:

```python
names = Set[String]()
```

Empty set literals still need an expected `Set[T]` type in the current bootstrap compiler:

```python
mut names: Set[String] = Set{}
```

Current set methods include:

- `len() -> int32`
- `is_empty() -> bool`
- `clone() -> Set[T]`
- `contains(value: T) -> bool`
- `insert(value: T) -> bool`
- `remove(value: T) -> bool`

Sets deduplicate repeated literal or inserted values, and they can be iterated by value or through an explicit shared borrow.

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## Literal Defaults

In the current compiler:

- integer literals default to `int32`
- floating-point literals default to `float64`
- duration literals such as `5ms`, `1s`, and `2m` have type `Duration`

Floating-point literals can also adopt an expected `float32` type when the surrounding annotation or signature provides it:

```python
ratio: float32 = 3.25
```

Negative literals are also supported:

```python
offset: int32 = -5
temperature: float64 = -3.5
```
