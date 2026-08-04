# Control Flow

Aura supports the standard control-flow constructs: conditionals, loops, pattern matching, and early exit.

## `if`, `elif`, and `else`

```aura
score: int32 = 90

if score < 50:
    print("low")
elif score < 80:
    print("mid")
else:
    print("high")
```

Conditions must evaluate to `bool`. Unlike Python, Aura does not support truthy or falsy coercions -- you must write explicit comparisons.

See [examples/control_flow/if_elif_else.au](../examples/control_flow/if_elif_else.au).

Aura supports boolean operators in conditions:

```aura
if ready and not blocked:
    print("ready")

allowed = is_admin or is_owner
```

See [examples/control_flow/boolean_logic.au](../examples/control_flow/boolean_logic.au).

## Conditional Expressions

Use `value if condition else alternative` when a branch chooses one value:

```aura
label = "ready" if ready else "waiting"
```

The condition is evaluated first and must be `bool`. Aura then evaluates
exactly one arm. Both arms must produce the same static type; an expected type
from a return, annotation, or call argument is used to type literals in both
arms.

Conditional expressions bind less tightly than `or` and associate to the
right. A nested expression therefore reads as an `if`/`elif` choice:

```aura
label = "high" if score >= 80 else "mid" if score >= 50 else "low"
```

Moving a non-copy value in either arm makes that value unavailable after the
conditional, because either runtime path may be selected.

See [examples/control_flow/conditional_expressions.au](../examples/control_flow/conditional_expressions.au).

## Membership Tests

`in` and `not in` ask whether a container holds a value:

```aura
ports = [80, 443]
print(443 in ports)
print(8080 not in ports)
```

The container decides what the test means and what the value must be:

| Container | Tests | Value must be |
| --- | --- | --- |
| `list[T]` | element membership | `T` |
| `set[T]` | element membership | `T` |
| `dict[K, V]` | key membership | `K` |
| `str` | substring containment | `str` |

Membership reads both operands and moves neither, so a non-copy container and
a non-copy value are both still usable afterwards. A container Aura cannot
test reports `AU2003`, and a value of the wrong type reports `AU2002`.

## Chained Comparisons

Comparisons chain the way they do in Python, so a range check reads as one
expression:

```aura
def in_range(value: int32, low: int32, high: int32) -> bool:
    return low <= value < high
```

`low <= value < high` means `low <= value and value < high`, except that
`value` is evaluated only once. The chain stops at its first false link, so the
operands after it are never evaluated. Equality, ordering, and membership all
chain at the same level, so `a == b < c` is also one chain.

The checker still checks every operand as if it were evaluated. A chain that
would move a value only on a path short-circuiting skips is rejected, which is
the same conservative rule the other branching forms use.

See [examples/control_flow/membership_and_chains.au](../examples/control_flow/membership_and_chains.au).

## `while`

```aura
mut n: int32 = 0
while n < 10:
    print(n)
    n += 1
```

Use `while true:` with `break` for loops with complex exit conditions:

```aura
mut attempts: int32 = 0
while true:
    attempts += 1
    if attempts >= 3:
        print("giving up")
        break
```

## `break` and `continue`

Both work inside `while` and `for` loops:

```aura
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

```aura
class Empty:
    pass

def noop():
    pass
```

See [examples/basics/pass_keyword.au](../examples/basics/pass_keyword.au).

## `for` Over `range`

```aura
mut total: int64 = 0

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

Vectors and sets can be iterated in three ownership modes. The choice matters because of Aura's ownership model (see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md)):

**Bare/default** -- reads through a shared borrow. The collection stays valid:

```aura
names = ["Ada", "Grace"]
for name in names:
    print(name)
print(names.len())       # still usable
```

**Owned** -- consumes the collection. After the loop, it is no longer valid:

```aura
names = ["Ada", "Grace"]
for name in own names:
    print(name)
# names is consumed -- cannot use it after this loop
```

`for name in names:` is the explicit spelling of shared iteration.

**By mutable borrow** -- modifies elements in place. Requires a `mut` binding:

```aura
mut scores = [1, 2, 3]
for item in mut scores:
    item += 1
# scores is now [2, 3, 4]
```

Use bare `for x in collection` for ordinary reads, `for x in own collection`
when you are done with it, and `for x in mut collection` when you need
to update list elements.

See [examples/collections/list_iteration.au](../examples/collections/list_iteration.au) and [examples/collections/list_polish.au](../examples/collections/list_polish.au).

Sets support bare shared and `own` iteration:

```aura
seen = {1, 2, 3}
for value in seen:
    print(value)
```

See [examples/collections/set_basics.au](../examples/collections/set_basics.au).

## `enumerate` And `zip`

`enumerate(...)` gives you the position alongside each value:

```aura
hosts = ["alpha", "beta"]
for index, host in enumerate(hosts):
    print(f"{index}: {host}")
```

`zip(...)` walks two sequences together and stops at the shorter one:

```aura
ports = [80, 443, 8080]
for host, port in zip(hosts, ports):
    print(f"{host}:{port}")
```

Both are compiler-known `for` loop forms. They do not produce values that can
be stored, so `pairs = enumerate(hosts)` is rejected with guidance to use the
loop spelling.
Both read their operands by position, so each one must be a `list[T]` or a
`set[T]`, and both iterate over the bare-loop shared default: no `own` or
`mut` modifier, the operands stay borrowed for the whole
loop, and a non-copy element binding cannot be moved out.

If you define your own `enumerate` or `zip` function, yours wins.

See [examples/control_flow/enumerate_and_zip.au](../examples/control_flow/enumerate_and_zip.au).

## Comprehensions

Comprehensions package nested bare loops and filters into an eager collection
expression:

```aura
values = [1, 2, 3, 4]
even_squares = [value * value for value in values if value % 2 == 0]
pairs = [
    left * 10 + right
    for left in values if left < 3
    for right in values if right < 3
]
```

The output expression is written first, but the first iterable runs first.
Filters run left to right and nested clauses are outer-major, so `pairs` is
`[11, 12, 21, 22]`. A dictionary comprehension evaluates its key before its value:

```aura
labels = {value: value * 10 for value in values if value >= 3}
```

Every clause uses the bare-loop contract. List and set sources are shared,
Range yields copy values, `enumerate`/`zip` keep their loop behavior, and
Queue receives owned items through its existing carve-out. Comprehension
targets do not leak outside the expression.

There is no `mut`/`own` clause spelling and no lazy generator expression. Use
an explicit loop when you need mutation, `break`, `continue`, or incremental
stream processing.

See
[examples/collections/comprehensions.au](../examples/collections/comprehensions.au).

## Current Limits

The current compiler supports `for` over:

- `range(stop)` and `range(start, stop)` with named-argument forms
- bare/`own` `list[T]`, plus `mut list[T]`
- bare/`own` `set[T]`
- `Queue[T]` (iterates until the queue closes)

It also supports the `enumerate(seq)` and `zip(first, second)` loop forms over
`list[T]` and `set[T]`.

Not yet supported:

- user-defined iterable protocols
- `enumerate` or `zip` over a `Range` or `Queue[T]`
- `mut set[T]`
- custom step values for `range`

Queue iteration is different: it receives each item already owned, and the
Queue handle is copyable. The explicit `own` and `mut` forms are rejected for
Queue; use `for item in queue:`.
