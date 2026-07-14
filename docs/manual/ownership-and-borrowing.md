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
- an `own` function parameter or an `own self` method receiver
- a by-value return
- a class or enum payload, collection literal, mutating collection method, or
  simple Map indexed assignment that stores the value
- by-value enum matching
- `own` iteration over `Vec[T]` or `Set[T]`
- the resource expression of `with`
- a task-start argument copied or moved into task-owned capture storage

An expression is evaluated before its move is recorded at that boundary. Aurora also rejects an expression that tries to borrow and move overlapping places in incompatible subexpressions.

## Borrow Forms

| Form | Meaning |
| --- | --- |
| `value: T` | By value for copy `T`; shared borrow for non-copy or unresolved generic `T`. |
| `value: own T` | Explicit owned ordinary parameter. |
| `value: borrow T` | Shared borrowed ordinary parameter. |
| `value: borrow mut T` | Exclusive mutable borrowed ordinary parameter. |
| `self` | Shared method receiver and the default receiver spelling. |
| `borrow self` | Explicit synonym for shared `self`. |
| `own self` | Consuming method receiver. |
| `borrow mut self` | Exclusive mutable method receiver. |
| `for value in collection:` | Default shared iteration for `Vec` and `Set`. |
| `for value in own collection:` | Consuming iteration for `Vec` and `Set`. |
| `for value in borrow collection:` | Shared-borrow iteration. |
| `for value in borrow mut collection:` | Mutable-borrow iteration where supported. |
| `match borrow value:` | Shared borrowed pattern matching. |
| `match borrow mut value:` | Mutable borrowed pattern matching with writeback. |
| `-> borrow[source] T` | Shared borrowed result from one declared source. |
| `-> borrow mut[source] T` | Mutable borrowed result from one mutable source. |

The spelling asymmetry is intentional: parameter ownership occupies the type position as `value: own T`, parallel to `value: borrow T`, while loop ownership prefixes the iterable as `for value in own values` because loops have no type position.

Call sites never prefix arguments with `borrow` or `own`. The parameter or receiver declaration selects the mode:

```python
def render(name: borrow String) -> String:
    return name.to_upper()

name = "aurora"
print(render(name))
print(name)
```

A shared borrow permits reading but cannot be moved and cannot be used as a mutable place. A mutable borrow is exclusive and may mutate its source through the borrowed binding.

Shared-borrow and `own` parameters may have defaults. An omitted shared default
creates a fresh temporary that lives through the call; an omitted owned default
creates a fresh value that the call consumes. A `borrow mut` parameter cannot
have a default, even for a copy type: its caller-invisible temporary would make
every mutation a silent lost write. Require the caller to pass a mutable value,
or take `own T` and return the result.

```python
def add_name(names: borrow mut Vec[String], name: own String):
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

Bare `Vec` and `Set` iteration retains the collection and yields shared-borrowed
non-copy elements. `for value in own collection` moves the collection once into
a loop-private source and yields owned elements. Reinitializing the consumed
source binding in the body cannot switch or truncate that active iteration.
That one-time source selection is accepted under ADR-0017; ADR-0006's
accepted loop ownership modes are unchanged.
`for value in borrow collection` is the explicit shared form.
`for value in borrow mut vec` requires a mutable vector place and yields
mutable-borrowed elements.

The place selected by bare or explicit borrowed iteration is frozen against
overlapping mutation for the loop body.
Mutable-borrow set iteration is not supported; mutate a set through `insert`
and `remove` outside borrowed iteration. Queue iteration receives values; it is
a scheduler operation, not a place traversal. The bare form copies the Queue
handle once at loop entry and yields owned items without freezing the source
binding; rebinding that source does not switch later receives. All three
explicit ownership modifiers are rejected. The one-time handle selection is
also accepted under ADR-0017. See
[Concurrency](/manual/concurrency).

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

`TaskGroup.start` and `start_soon` accept named functions or associated methods
with default-mode, `own`, or explicit shared-borrow parameters. `borrow mut`
targets are rejected.

```python
def worker(label: borrow String):
    print(label)

with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label.clone())
    print(label)
```

Each task argument is copied or moved into task-owned capture storage before
the child runs. The target then borrows or consumes that capture according to
its declared mode; a default non-copy parameter is a shared borrow from the
capture. Copy task and queue handles still refer to shared runtime state. See
[Concurrency](/manual/concurrency) and [Execution Model](/manual/execution-model#tasks-and-scheduler).

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

## Grammar

The normative ownership spellings are parameter and return type-position
`own`, `borrow`, `borrow mut`, `borrow[source]`, and
`borrow mut[source]`; the four receiver forms; loop-prefix `own`, `borrow`,
and `borrow mut`; `match borrow` and
`match borrow mut`; mutable bindings; and `with`. Their productions are in
[Grammar](/manual/grammar). Call arguments themselves never carry an ownership
prefix.

## Typing Rules

Every expression has one static copy/move category and every parameter has one
declaration-stable passing mode. Bare copy parameters pass by value; bare
non-copy and unresolved generic parameters share-borrow; explicit `own`
consumes; `borrow mut` requires one exclusive mutable place. Shared and owned
defaults are legal, with shared temporaries lasting through the call;
`borrow mut` defaults are rejected. Place-prefix overlap, partial moves,
control-flow joins, loop repetition, borrowed-return provenance, borrowed
matches, borrowed iteration, task capture, and managed-resource containment are
checked before lowering.

## Runtime Semantics

A copy use duplicates a value and a move transfers it. Shared and mutable
borrows are statically enforced access contracts rather than first-class
runtime reference values in Aurora 0.1. Mutable borrowed calls and Vec
iteration write through the original place; `match borrow mut` reconstructs
and writes back on normal arm exit. Simple Map indexed assignment accepts and
owns any value type; direct compound indexed assignment requires a copy `Vec`
element or `Map` value.
Task start first transfers captures into child-owned storage. `with` owns one cleanup registration and runs it exactly
once on every maintained scope exit under the documented failure-precedence
rules.

## Ownership And Evaluation Order

Subexpressions evaluate in the order defined by [Execution
Model](/manual/execution-model#evaluation-order), then a copy, move, or borrow
is applied at its typed boundary. All receiver and argument accesses for one
call are checked together, so source order cannot legalize overlapping shared,
mutable, and owned uses. A partial move preserves proven-disjoint fields;
reinitializing the exact moved place restores it. Control-flow merging never
silently restores ownership, and Aurora never inserts a clone or coercion to
repair an invalid use.

Capturing a copy place duplicates its value. A non-copy place selected as a
binary left operand, index base, method receiver, or indexed-assignment target
remains borrowed until that operation consumes all of its inputs. A later
shared borrow is permitted. An overlapping mutable borrow or consumption is
rejected with `AU3002`, with the retained selection identified as the borrow
origin. Name roots and projected member places follow the same rule, and no
backend inserts a hidden deep clone. Operations that require a point-in-time
representation produce it immediately; each f-string interpolation renders to
`String` before the next interpolation begins.

Compound assignment uses the corresponding binary operator dispatch, including
applicable user-defined operator traits for root and projected targets. A copy
target is captured before the right operand. A non-copy root or projected
target remains borrowed across that operand, so overlapping mutable borrow or
consumption is `AU3002`. A non-copy `Vec` element or `Map` value cannot be a
direct compound target until live aliases exist; Aurora rejects the operation
instead of cloning or destructively moving the stored value.

## Diagnostics

`AU1101` reports malformed ownership, receiver, loop, match-borrow, or return
syntax. `AU2002` covers type and provenance-source type mismatch, while
`AU2004` reports argument binding that cannot satisfy a required mutable place.
`AU2999` covers unsupported move/control-flow/resource cases without a narrower
category. `AU3001`
reports use of a moved or partially moved place. `AU3002`
reports overlapping or invalid borrows, moving through a borrow, borrowed-
return provenance/materialization failure, invalid mutable-borrow defaults or
task targets, stale borrowed-pattern bindings, and later mutable or consuming
access that overlaps a retained non-copy binary operand, index base, method
receiver, or indexed-assignment target. In a retained-expression conflict, the
diagnostic points to both the later access and the retained-borrow origin.
`AU3003` reports assignment or mutation through an immutable place, including
shared `self`. `AU3004`
reports invalid parameter, receiver, loop, or Queue-iteration ownership modes.
`AU3005` rejects a direct indexed read of a non-copy Vec element or Map value;
`AU3006` rejects the corresponding indexed compound read-modify-write.
Ownership failures are static. A runtime operation reached through an owned or
borrowed value keeps its own code: `AU4001` for a general trap, `AU4002` for
arithmetic overflow or underflow, `AU4003` for a bounds or lookup violation,
`AU4004` for a zero divisor, and `AU4005` for a resource or I/O failure.

## Backend Support

The compiler performs one ownership/borrow analysis before backend selection.
MIR execution and direct native generation receive the same resolved parameter
ABI, moves, copies, capture modes, borrowed-match/iteration operations, and
cleanup registrations. Analysis and LSP signatures expose those same modes.
The parity matrix pins observable move, mutation, capture, writeback, cleanup,
and primary-diagnostic behavior.

## Limits And Implementation-Defined Behavior

Place analysis tracks local roots and field-prefix paths; it proves disjoint
sibling fields but is not a general alias theorem. Non-copy borrowed-return
declarations are provenance-checked but calls are contained until live alias
storage lands. Mutable Set iteration, explicit Queue ownership modifiers,
mutable-borrow task targets, moving out of a managed resource, and arbitrary
reference values are unavailable. Loop move analysis intentionally uses only
the limited Boolean reasoning described above. Ownership mode and evaluation
order are language-defined, not backend- or host-defined.

## Status

Copy/move classification, declaration-stable parameter defaults, explicit
owned/shared/mutable passing, all receiver modes, call-boundary exclusivity,
partial moves and reinitialization, flow-sensitive checks, borrowed-return
containment, borrowed matching and Vec/Set iteration, task capture, cloning,
and lexical resource ownership are implemented for the post-Phase 1.5
surface; the one-time Vec/Set/Queue iteration-source rule is accepted under
ADR-0017. Live non-copy borrowed aliases and their runtime storage are reserved
for Phase 6. General reference values outside that reserved contract, mutable
Set iteration, Queue ownership modifiers, and mutable-borrow task capture are
unavailable.
