# Control Flow

This chapter covers conditionals, comparisons, loops, early exit, and
comprehensions. Pattern matching with `match` has its own chapter,
[09-enums-and-match.md](09-enums-and-match.md).

## `if`, `elif`, and `else`

```aura check-pass
score: int32 = 90

if score < 50:
    print("low")
elif score < 80:
    print("mid")
else:
    print("high")
```

This prints `high`. A condition must be a `bool`. Unlike Python, Aura has no
truthy or falsy coercion, so you write the comparison explicitly.

See [examples/control_flow/if_elif_else.au](../examples/control_flow/if_elif_else.au).

Conditions can use the boolean operators `and`, `or`, and `not`:

```aura fragment
if ready and not blocked:
    print("ready")

allowed = is_admin or is_owner
```

See [examples/control_flow/boolean_logic.au](../examples/control_flow/boolean_logic.au).

## Conditional Expressions

Use `value if condition else alternative` when a branch chooses one value:

```aura fragment
label = "ready" if ready else "waiting"
```

Aura evaluates the condition first, and it must be a `bool`. Then Aura
evaluates exactly one arm. Both arms must have the same static type. An
expected type from a return, annotation, or call argument types the literals
in both arms.

A conditional expression binds less tightly than `or` and groups to the
right. A nested one reads as an `if`/`elif` chain:

```aura fragment
label = "high" if score >= 80 else "mid" if score >= 50 else "low"
```

Either arm may run, so moving a non-copy value in either arm makes that value
unavailable after the expression.

See [examples/control_flow/conditional_expressions.au](../examples/control_flow/conditional_expressions.au).

## Membership Tests

`in` and `not in` ask whether a container holds a value:

```aura check-pass
ports = [80, 443]
print(443 in ports)
print(8080 not in ports)
```

This prints `true` twice. The container decides what the test means and what
type the value must have:

| Container | Tests | Value must be |
| --- | --- | --- |
| `list[T]` | element membership | `T` |
| `set[T]` | element membership | `T` |
| `dict[K, V]` | key membership | `K` |
| `str` | substring containment | `str` |

A membership test reads both operands and moves neither. A non-copy container
and a non-copy value both stay usable afterwards.

| Code | Cause |
| --- | --- |
| `AU2008` | The element or key type has no equality. Closures, `random.Rng`, opaque FFI handles, and values containing them have none. |
| `AU2003` | Aura cannot test membership in this container type. |
| `AU2002` | The value has the wrong type for the container. |

## Chained Comparisons

Comparisons chain as they do in Python, so a range check is one expression:

```aura check-pass
def in_range(value: int32, low: int32, high: int32) -> bool:
    return low <= value < high
```

`low <= value < high` means `low <= value and value < high`, except that
`value` is evaluated once. The chain stops at its first false link and skips
the operands after it. Equality, ordering, and membership all chain at the
same level, so `a == b < c` is also one chain.

The checker still checks every operand as if it runs. It rejects a chain that
would move a value only on a path that short-circuiting can skip. The other
branching forms use the same conservative rule.

See [examples/control_flow/membership_and_chains.au](../examples/control_flow/membership_and_chains.au).

## `while`

A `while` loop repeats while its condition is `true`. This one prints `0`
through `9`:

```aura check-pass
mut n: int32 = 0
while n < 10:
    print(n)
    n += 1
```

For a loop with a complex exit condition, use `while true:` with `break`:

```aura check-pass
mut attempts: int32 = 0
while true:
    attempts += 1
    if attempts >= 3:
        print("giving up")
        break
```

## `break` and `continue`

Both work inside `while` and `for` loops. `continue` skips to the next
iteration and `break` leaves the loop. This loop prints `1`, `3`, `5`, and `7`:

```aura check-pass
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

Use `pass` when a block must exist but has no statements, as in Python:

```aura check-pass
class Empty:
    pass

def noop():
    pass
```

See [examples/basics/pass_keyword.au](../examples/basics/pass_keyword.au).

## `for` Over `range`

`range(stop)` counts from `0` to `stop - 1`. `range(start, stop)` counts from
`start` to `stop - 1`:

```aura check-pass
mut total: int64 = 0

for value in range(6):
    if value == 3:
        continue
    if value == 5:
        break
    total += value
```

This loop adds `0`, `1`, `2`, and `4`, so `total` ends at `7`.

See [examples/control_flow/for_range.au](../examples/control_flow/for_range.au).

## `for` Over Collections

A `for` loop over a list or set has an ownership mode. The mode decides what
happens to the collection. See
[06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for the
ownership model.

| Spelling | Mode | Lists | Sets |
| --- | --- | --- | --- |
| `for x in collection` | shared borrow, for ordinary reads | yes | yes |
| `for x in own collection` | consumes the collection, when you are done with it | yes | yes |
| `for x in mut collection` | mutable borrow, to update elements | yes | no |

**Shared.** The bare form reads through a shared borrow. The collection stays
valid:

```aura check-pass
names = ["Ada", "Grace"]
for name in names:
    print(name)
print(names.len())       # still usable
```

**Owned.** `own` consumes the collection. You cannot use it after the loop:

```aura check-pass
def main():
    names = ["Ada", "Grace"]
    for name in own names:
        print(name)
    # names is consumed -- cannot use it after this loop
```

**Mutable.** `mut` changes elements in place. It needs a `mut` binding:

```aura check-pass
mut scores = [1, 2, 3]
for item in mut scores:
    item += 1
# scores is now [2, 3, 4]
```

See [examples/collections/list_iteration.au](../examples/collections/list_iteration.au) and [examples/collections/list_polish.au](../examples/collections/list_polish.au).

Sets support the bare shared and `own` forms:

```aura check-pass
seen = {1, 2, 3}
for value in seen:
    print(value)
```

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## `enumerate` And `zip`

`enumerate(...)` gives you the position alongside each value. This prints
`0: alpha` and `1: beta`:

```aura check-pass
hosts = ["alpha", "beta"]
for index, host in enumerate(hosts):
    print(f"{index}: {host}")
```

`zip(...)` walks two sequences together and stops at the shorter one:

```aura fragment
ports = [80, 443, 8080]
for host, port in zip(hosts, ports):
    print(f"{host}:{port}")
```

Both are `for` loop forms that the compiler knows. They do not produce values
you can store. The compiler rejects `pairs = enumerate(hosts)` and suggests
the loop spelling instead.

Both read their operands by position, so each operand must be a `list[T]` or
a `set[T]`. Both use the bare shared loop mode:

- there is no `own` or `mut` modifier
- the operands stay borrowed for the whole loop
- a non-copy element binding cannot be moved out

If you define your own `enumerate` or `zip` function, yours wins.

See [examples/control_flow/enumerate_and_zip.au](../examples/control_flow/enumerate_and_zip.au).

## Comprehensions

A comprehension packs nested bare loops and filters into one expression that
builds a collection eagerly:

```aura check-pass
values = [1, 2, 3, 4]
even_squares = [value * value for value in values if value % 2 == 0]
pairs = [
    left * 10 + right
    for left in values if left < 3
    for right in values if right < 3
]
```

You write the output expression first, but the first iterable runs first.
Filters run left to right. Nested clauses run outer-major, so `pairs` is
`[11, 12, 21, 22]`. A dictionary comprehension evaluates its key before its
value:

```aura fragment
labels = {value: value * 10 for value in values if value >= 3}
```

Every clause follows the bare loop rules:

- list and set sources are shared
- a Range yields copy values
- `enumerate` and `zip` behave as they do in loops
- a Queue source receives owned items, the same exception as in loops

A comprehension's loop variables do not leak outside the expression.

There is no `mut` or `own` clause and no lazy generator expression. Use an
explicit loop when you need mutation, `break`, `continue`, or incremental
stream processing.

See
[examples/collections/comprehensions.au](../examples/collections/comprehensions.au).

## Current Limits

The compiler supports `for` over:

- `range(stop)` and `range(start, stop)`, including the named-argument forms
- `list[T]` in bare, `own`, and `mut` form
- `set[T]` in bare and `own` form
- `Queue[T]`, which iterates until the queue closes

It also supports the `enumerate(seq)` and `zip(first, second)` loop forms over
`list[T]` and `set[T]`.

Not implemented:

- user-defined iterable protocols
- `enumerate` or `zip` over a `Range` or `Queue[T]`
- `mut set[T]`
- custom step values for `range`

Queue iteration is different from list and set iteration. It receives each
item already owned, and the Queue handle is copyable. The compiler rejects the
`own` and `mut` forms for Queue. Use `for item in queue:`.
