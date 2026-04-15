# Control Flow

Aurora supports the standard control-flow constructs: conditionals, loops, pattern matching, and early exit.

## `if`, `elif`, and `else`

```python
score: int32 = 90

if score < 50:
    print("low")
elif score < 80:
    print("mid")
else:
    print("high")
```

Conditions must evaluate to `bool`. Unlike Python, Aurora does not support truthy or falsy coercions -- you must write explicit comparisons.

See [examples/control_flow/if_elif_else.au](../examples/control_flow/if_elif_else.au).

Aurora supports boolean operators in conditions:

```python
if ready and not blocked:
    print("ready")

allowed = is_admin or is_owner
```

See [examples/control_flow/boolean_logic.au](../examples/control_flow/boolean_logic.au).

## `while`

```python
mut n: int32 = 0
while n < 10:
    print(n)
    n += 1
```

Use `while true:` with `break` for loops with complex exit conditions:

```python
mut attempts: int32 = 0
while true:
    attempts += 1
    if attempts >= 3:
        print("giving up")
        break
```

## `break` and `continue`

Both work inside `while` and `for` loops:

```python
mut n: int32 = 0
while n < 10:
    n += 1
    if n % 2 == 0:
        continue       # skip even numbers
    if n > 7:
        break          # stop after 7
    print(n)
```

See [examples/control_flow/while_break_continue.au](../examples/control_flow/while_break_continue.au).

## `pass`

Use `pass` when a block must exist but has no statements. This is the same as Python:

```python
class Empty:
    pass

def noop():
    pass
```

See [examples/basics/pass_keyword.au](../examples/basics/pass_keyword.au).

## `for` Over `range`

```python
mut total: int32 = 0

for value in range(6):
    if value == 3:
        continue
    if value == 5:
        break
    total += value
```

`range(stop)` counts from `0` to `stop - 1`. `range(start, stop)` counts from `start` to `stop - 1`.

See [examples/control_flow/for_range.au](../examples/control_flow/for_range.au).

## `for` Over Collections

Vectors and sets can be iterated in three ways. The choice matters because of Aurora's ownership model (see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md)):

**By value** -- consumes the collection. After the loop, the collection is no longer valid:

```python
names = ["Ada", "Grace"]
for name in names:
    print(name)
# names is consumed -- cannot use it after this loop
```

**By shared borrow** -- reads without consuming. The collection stays valid:

```python
names = ["Ada", "Grace"]
for name in borrow names:
    print(name)
print(names.len())       # still usable
```

**By mutable borrow** -- modifies elements in place. Requires a `mut` binding:

```python
mut scores = [1, 2, 3]
for item in borrow mut scores:
    item += 1
# scores is now [2, 3, 4]
```

Use `for x in borrow collection` as the default when you want to keep the collection. Use `for x in collection` only when you are done with it. Use `for x in borrow mut collection` when you need to update elements.

See [examples/collections/vec_iteration.au](../examples/collections/vec_iteration.au) and [examples/collections/vec_polish.au](../examples/collections/vec_polish.au).

Sets support by-value and shared-borrow iteration:

```python
seen = Set{1, 2, 3}
for value in borrow seen:
    print(value)
```

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## Current Limits

The current compiler supports `for` over:

- `range(stop)` and `range(start, stop)` with named-argument forms
- `Vec[T]`, `borrow Vec[T]`, and `borrow mut Vec[T]`
- `Set[T]` and `borrow Set[T]`
- `Channel[T]` (iterates until the channel closes)

Not yet supported:

- user-defined iterable protocols
- `borrow mut Set[T]`
- custom step values for `range`
