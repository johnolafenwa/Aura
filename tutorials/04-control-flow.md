# Control Flow

Aurora’s implemented control-flow subset is already large enough for useful scripts and small programs.

## `if`, `elif`, and `else`

```python
def main():
    score: int32 = 90

    if score < 50:
        print("low")
    elif score < 80:
        print("mid")
    else:
        print("high")
```

See [examples/control_flow/if_elif_else.au](../examples/control_flow/if_elif_else.au).

Conditions must evaluate to `bool`. The bootstrap compiler does not support Python-style truthy or falsy coercions.

Aurora also supports boolean operators directly in conditions and expressions:

```python
if ready and not blocked:
    print("ready")

allowed = is_admin or is_owner
```

See [examples/control_flow/boolean_logic.au](../examples/control_flow/boolean_logic.au).

## `while`

```python
while n < 10:
    n += 1
```

## `break` and `continue`

Aurora supports both inside loops:

```python
if n % 2 == 0:
    continue

if n > 7:
    break
```

See [examples/control_flow/while_break_continue.au](../examples/control_flow/while_break_continue.au).

## `pass`

Use `pass` when a block is intentionally empty:

```python
class Empty:
    pass

def noop():
    pass
```

See [examples/basics/pass_keyword.au](../examples/basics/pass_keyword.au).

## `for`, `range`, `Vec[T]`, And `Set[T]`

Aurora now supports `for` loops over `range(...)`, `Vec[T]`, and `Set[T]` values.

```python
mut total: int32 = 0

for value in range(6):
    if value == 3:
        continue
    if value == 5:
        break
    total += value
```

See [examples/control_flow/for_range.au](../examples/control_flow/for_range.au).

Vectors can be iterated by value, through an explicit shared borrow, or through an explicit mutable borrow:

```python
mut total = 0
for value in values:
    total += value

for name in borrow names:
    print(name)

mut scores = [1, 2, 3]
for item in borrow mut scores:
    item += 1
```

See [examples/collections/vec_iteration.au](../examples/collections/vec_iteration.au) and [examples/collections/vec_polish.au](../examples/collections/vec_polish.au).

As with `push(...)`, `set(...)`, and other mutating vector operations, `borrow mut` iteration requires the vector place itself to be mutable.

Sets support by-value and shared-borrow iteration:

```python
seen = Set{1, 2, 3}
for value in borrow seen:
    print(value)
```

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## Current Limits

The current compiler supports `for` over:

- `range(stop)`
- `range(start, stop)`
- the corresponding named-argument forms
- `Vec[T]`
- `borrow Vec[T]`
- `borrow mut Vec[T]`
- `Set[T]`
- `borrow Set[T]`
- `Channel[T]`

It does not yet support:

- user-defined iterable protocols
- `borrow mut Set[T]`
- custom step values for `range`
- `range(...)` bounds outside the current signed index space
