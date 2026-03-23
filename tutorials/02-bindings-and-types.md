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
- `Channel[T]`
- `Task[T]`
- `TaskGroup`
- `Option[T]`
- `Result[T, E]`
- `SendError[T]`

For integer types, the current checker and runtimes both enforce the annotated width. A binding like `value: int8 = 127` is valid, but pushing that value out of range later will fail with a runtime diagnostic instead of silently widening it.

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
