# Ownership And Borrowing

Aurora statically tracks whether an operation copies, moves, shares, or mutates a value. The rules apply to local bindings, parameters, method receivers, fields, supported indexed operations, collection iteration, pattern matching, task starts, and resources.

A **place** is a storage location such as a local binding or field path. A copy use duplicates a value. A move use transfers ownership from a place. A borrow temporarily grants access without transferring ownership.

## Copy Types

Copy values are duplicated by assignment, by-value argument passing, returns, collection insertion, and other value uses. The source remains usable.

Current copy categories are:

- all signed and unsigned integer types
- `float32` and `float64`
- `bool`
- `Duration`
- `Queue[T]` and `Task[T]` handles
- `copy class` values whose fields are copyable
- user enums whose every declared payload type is statically copyable
- `Option[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` when every payload type is copyable

```python
a = 1
b = a
print(a)
print(b)
```

`Queue[T]` and `Task[T]` are copy handles to shared runtime state. Copying one does not duplicate the underlying queue, task, queued values, or stored result.

## Move Types

Move values transfer ownership on by-value use. Current move categories include:

- `String`
- `Vec[T]`, `Map[K, V]`, and `Set[T]`
- ordinary user classes
- user or builtin enums with any move payload
- `TaskResult[T]`, `WaitAny[T]`, and `WaitAll[T]` even when `T` is copyable
- `Range`
- `TaskGroup`
- file, process, supervisor, pipe, and network resources

```python
name = "aurora"
other = name
print(other)
# print(name) would be rejected: name was moved
```

A generic payload whose declared type is an unconstrained type parameter is not assumed copyable. The canonical category list and builtin generic types are in [Types](/manual/types#copy-and-move-categories).

## Operations That Move

A non-copy value is consumed when used in an owned position, including:

- assignment into a new owned binding
- a by-value function parameter or an `own self` method receiver
- a by-value return
- a class or enum payload, collection literal, or mutating collection method that stores the value
- by-value enum matching
- by-value iteration over `Vec[T]` or `Set[T]`
- the resource expression of `with`
- a task-start argument for a by-value target parameter

An expression is evaluated before its move is recorded at that boundary. Aurora also rejects an expression that tries to borrow and move overlapping places in incompatible subexpressions.

## Borrow Forms

| Form | Meaning |
| --- | --- |
| `value: borrow T` | Shared borrowed ordinary parameter. |
| `value: borrow mut T` | Exclusive mutable borrowed ordinary parameter. |
| `self` | Shared method receiver and the default receiver spelling. |
| `borrow self` | Explicit synonym for shared `self`. |
| `own self` | Consuming method receiver. |
| `borrow mut self` | Exclusive mutable method receiver. |
| `for value in borrow collection:` | Shared-borrow iteration. |
| `for value in borrow mut collection:` | Mutable-borrow iteration where supported. |
| `match borrow value:` | Shared borrowed pattern matching. |
| `match borrow mut value:` | Mutable borrowed pattern matching with writeback. |
| `-> borrow[source] T` | Shared borrowed result from one declared source. |
| `-> borrow mut[source] T` | Mutable borrowed result from one mutable source. |

Call sites never prefix arguments with `borrow` or `own`. The parameter or receiver declaration selects the mode:

```python
def render(name: borrow String) -> String:
    return name.to_upper()

name = "aurora"
print(render(name))
print(name)
```

A shared borrow permits reading but cannot be moved and cannot be used as a mutable place. A mutable borrow is exclusive and may mutate its source through the borrowed binding.

```python
def add_name(names: borrow mut Vec[String], name: String):
    names.push(name)

mut names = Vec[String]()
add_name(names, "Ada")
```

Only a mutable place can satisfy `borrow mut`. A local becomes mutable with `mut`; a field is mutable when its base place is mutable; a `borrow mut` receiver or parameter is a mutable place inside its body. Parameter bindings themselves are not reassigned.

## Call-Boundary Exclusivity

All receiver and argument accesses for one call are checked together. Shared borrows may overlap other shared borrows. Every mutable borrow and every move must be exclusive with respect to an overlapping place.

```python
class Acc:
    value: int32

    def add_from(borrow mut self, source: borrow Acc):
        self.value += source.value

mut acc = Acc(value=1)
# acc.add_from(acc) is rejected: mutable self overlaps shared source
```

Place overlap is prefix-based for tracked name/field paths. `value` overlaps `value.field`, and `value.field` overlaps `value.field.inner`. Distinct roots do not overlap. Sibling fields such as `pair.left` and `pair.right` are distinct when the checker can prove those paths.

The same exclusivity rule applies when one argument consumes a value and another argument borrows it. Argument evaluation order does not make an otherwise overlapping call legal.

## Partial Moves And Reinitialization

Moving a non-copy field from an owned class marks that field path moved while preserving disjoint fields:

```python
class User:
    name: String
    id: int32

mut user = User(name="Ada", id=1)
name = user.name
print(user.id)

user.name = "Grace"
print(user.name)
```

The complete class value cannot be used while any field remains moved. Assigning the exact moved field reinitializes that path. Assigning a fully moved mutable binding reinitializes the binding and clears its moved-field state.

Moving a non-copy field through a shared or mutable borrow is rejected because the borrower does not own the containing value:

```python
def bad(user: borrow User) -> String:
    return user.name # rejected
```

Use `.clone()` for a new owned value when the type supports it, or expose an owner method that performs the read or mutation:

```python
def good(user: borrow User) -> String:
    return user.name.clone()
```

## Flow-Sensitive Move Checking

Branches and match arms are checked independently. At a reachable join, a binding or field is considered moved if it may have been moved on any incoming path unless it was definitely reinitialized on all relevant paths.

Moves inside a loop need an additional invariant: the loop may execute again. Aurora rejects a first move or partial move from an outer value in a repeatable loop when the next iteration could reuse the moved place. Limited constant-boolean reasoning recognizes forms based on `true`, `false`, grouping, and `not`; programs should not depend on broader compile-time evaluation.

Block-local bindings do not escape their branch, arm, loop, or `with` body. See [Names And Scopes](/manual/names-and-scopes#block-scope-and-control-flow).

## Borrowed Returns And Provenance

A borrowed-return signature identifies which receiver or parameter is the source:

```python
def identity(value: borrow[source] int32) -> borrow[source] int32:
    return value
```

The returned expression must derive from the selected source. A source may be named by its parameter name, `self`, or a borrow label. When exactly one eligible source exists, it may be inferred; multiple eligible sources require an explicit selection.

Shared borrowed-return declarations may derive from shared or mutable borrows. Mutable borrowed-return declarations may derive only from mutable borrows. A call returning a copy type becomes an ordinary copied value. Aurora 0.1 rejects calls producing non-copy borrowed results because neither maintained backend has live alias storage yet; return an owned clone or expose an owner method instead.

Borrow labels describe source equivalence across a call signature. They do not create arbitrary reference values, permit returning a local owned non-copy temporary, or extend the lifetime of a source. Non-copy declarations remain checked for provenance so the reserved contract is stable for Phase 6. The detailed signature rules are in [Functions](/manual/functions#borrowed-returns).

## Borrowed Pattern Matching

By-value matching consumes a non-copy enum scrutinee. `match borrow` retains the enum and gives non-copy payload bindings shared-borrow provenance:

```python
result: Result[String, String] = Result.Ok("ready")

match borrow result:
    case Result.Ok(value):
        print(value)
    case Result.Err(error):
        print(error)
```

`match borrow mut` requires a mutable place. Its non-copy payload bindings are mutable borrows, and mutations are written back by reconstructing the enum on normal arm exit. A nested mutable match cannot overlap an already active mutable match. Reassigning the exact scrutinee, its root, or an ancestor field invalidates payload bindings tied to the old value. A write to a proven-disjoint sibling field does not invalidate them.

Payload bindings are arm-local and cannot shadow a visible binding. Match typing and exhaustiveness are specified in [Enums And Pattern Matching](/manual/enums-and-match).

## Borrowed Iteration

By-value `Vec` and `Set` iteration consumes the collection and yields owned elements. `for value in borrow collection` retains it and yields shared-borrowed non-copy elements. `for value in borrow mut vec` requires a mutable vector place and yields mutable-borrowed elements.

The iterated place is frozen against overlapping mutation for the loop body. Mutable-borrow set iteration is not supported; mutate a set through `insert` and `remove` outside borrowed iteration. Queue iteration is a consuming/scheduler operation with separate rules in [Concurrency](/manual/concurrency).

## Clone

`.clone()` explicitly creates another owned structural value where the maintained type exposes cloning:

```python
name = "aurora"
copy = name.clone()
print(name)
print(copy)
```

String and collection clones copy their owned contents. Cloning runtime-backed resource handles does not necessarily create an independent host resource; rely on the resource's documented API rather than assuming deep host duplication.

## Tasks And Borrowing

`TaskGroup.start` and `start_soon` accept only named functions or associated methods whose ordinary parameters are all by value. Borrowed target parameters are rejected because a child may outlive the starting call frame.

```python
def worker(label: String):
    print(label)

with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label.clone())
    print(label)
```

Each task argument is copied or moved under ordinary call rules before the child runs. Copy task and queue handles still refer to shared runtime state. See [Concurrency](/manual/concurrency) and [Execution Model](/manual/execution-model#tasks-and-scheduler).

## Resources And `with`

Resource ownership should normally be lexical:

```python
import fs
import io

def show_file() -> Result[None, io.Error]:
    with file = try fs.open("data.txt"):
        text = try file.read_all()
        print(text)
    return Result.Ok(None)
```

`with` consumes the resource expression and creates a fresh mutable managed binding. A managed resource or its non-copy fields cannot be moved out in a way that would prevent cleanup. The registered `close` runs on normal fallthrough, `return`, escaping loop control, `try` propagation, and maintained runtime failure; nested cleanups run in reverse order.

Builtin resource behavior is defined by its module chapter. A user class must be non-generic and define `close(borrow mut self) -> None` with no ordinary parameters. Full cleanup ordering and failure precedence are specified in [Execution Model](/manual/execution-model#resource-lifetime-and-cleanup).
