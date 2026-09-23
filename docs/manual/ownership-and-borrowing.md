# Ownership And Borrowing

This page covers how Aura statically tracks whether an operation copies, moves, shares, or mutates a value. The rules apply to local bindings, parameters, method receivers, fields, supported indexed operations, collection iteration, pattern matching, task starts, and resources.

The page uses four terms:

- **A place** is a storage location, such as a local binding or a field path.
- **A copy use** duplicates a value.
- **A move use** transfers ownership from a place.
- **A borrow** grants temporary access without transferring ownership.

## Copy Types

A copy value is duplicated by assignment, by-value argument passing, returns, collection insertion, and other value uses. The source stays usable.

```aura
a = 1
b = a
print(a)
print(b)
```

The copy categories are:

- all signed and unsigned integer types
- `float32` and `float64`
- `bool`
- `Duration`
- `Queue[T]` handles, and `Task[T]` handles whose result type is repeatable
- tuples whose every element type is copyable
- `copy class` values whose fields are copyable
- user enums whose every declared payload type is statically copyable
- `Lookup[T]`, `Poll[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` when every payload type is copyable

`Queue[T]` is a copy handle to shared runtime state. `Task[T]` is copyable only when `T` is repeatable. Copying an allowed handle does not duplicate the underlying queue, task, queued values, or stored result.

## Move Types

A move value transfers ownership on by-value use.

```aura
def main():
    name = "aura"
    other = name
    print(other)
    # print(name) would be rejected: name was moved
```

The move categories include:

- tuples with at least one move element
- `str`
- `list[T]`, `dict[K, V]`, and `set[T]`
- `random.Rng`
- ordinary user classes
- user or builtin enums with any move payload
- `TaskResult[T]`, `SelectOutcome[Q, T]`, `WaitAny[T]`, and `WaitAll[T]`, even
  when every payload type is copyable
- `Range`
- `TaskGroup`
- file, process, supervisor, pipe, and network resources

A generic payload whose declared type is an unconstrained type parameter is not assumed copyable. [Types](/manual/types#copy-and-move-categories) has the canonical category list and the builtin generic types.

## Operations That Move

A non-copy value is consumed when it is used in an owned position. Owned positions include:

- assignment into a new owned binding
- an `own` function parameter or an `own self` method receiver
- a by-value return
- a class or enum payload, collection literal, mutating collection method, or
  simple dict indexed assignment that stores the value
- by-value enum matching
- `own` iteration over `list[T]` or `set[T]`
- the resource expression of `with`
- a task-start argument copied or moved into task-owned capture storage

Aura evaluates an expression before it records the move at that boundary. It rejects an expression that tries to borrow and move overlapping places in incompatible subexpressions.

List slicing is a clone-producing shared read. It does not move elements.

- `values[start:end]` retains `values` while the endpoint expressions run. It then copies Copy elements, or clones clone-safe non-Copy elements, into a fresh owned list.
- A type containing `random.Rng` cannot be sliced, because it cannot be safely duplicated. A non-repeatable Task observation right cannot be duplicated either.
- String slicing copies a Unicode-scalar range into a fresh owned `str`.

Neither result aliases its source or acts as an assignable place. After creation, the fresh result follows the ordinary move rules for any other owned non-copy list or `str` value.

Unpacking a non-copy tuple is one whole-source move. The target leaves receive owned elements. The source does not become a set of independently reusable positional partial-move places, so a later use of the source is rejected with the ordinary move diagnostic. Unpacking a copy tuple copies its elements and keeps the source usable.

## Borrow Forms

The declaration selects the mode. Call sites never prefix arguments with a capability:

```aura
def render(name: str) -> str:
    return name.to_upper()

name = "aura"
print(render(name))
print(name)
```

| Form | Meaning |
| --- | --- |
| `value: T` | Shared access for every `T`. As an implementation detail, copy bits may be passed directly. |
| `value: own T` | Explicit owned ordinary parameter. |
| `value: mut T` | Exclusive mutable borrowed ordinary parameter. |
| `self` | Shared method receiver and the default receiver spelling. |
| `own self` | Consuming method receiver. |
| `mut self` | Exclusive mutable method receiver. |
| `for value in collection:` | Default shared iteration for `list` and `set`. |
| `for value in own collection:` | Consuming iteration for `list` and `set`. |
| `for value in mut collection:` | Mutable-borrow iteration where supported. |
| `match value:` | Shared borrowed pattern matching. |
| `match own value:` | Consuming pattern matching. |
| `match mut value:` | Mutable borrowed pattern matching with writeback. |
| `-> T` | Owned result. A copy result is an ordinary independent copy. |
| `view name = place` | Shared view whose lifetime is inferred from its final use. |
| `view mut name = place` | Exclusive mutable write-through view. |
| `-> view T from source` | Shared returned view tied to one receiver or parameter. |
| `-> view mut T from source` | Mutable returned view tied to one mutable receiver or parameter. |
| `def(...) -> view [mut] T from name` | Stored callable contract whose result is a view of the named parameter. See [Closures](/manual/closures#stored-view-contracts). |

The spelling asymmetry is intentional. Parameter ownership sits in the type position, as in `value: own T`, parallel to `value: T`. Loop ownership prefixes the iterable, as in `for value in own values`, because loops have no type position.

A shared borrow permits reading. It cannot be moved and cannot be used as a mutable place. A mutable borrow is exclusive and can mutate its source through the borrowed binding:

```aura
def add_name(names: mut list[str], name: own str):
    names.append(name)

def main():
    mut names = list[str]()
    add_name(names, "Ada")
```

Only a mutable place can satisfy `mut T`:

- A local becomes mutable with `mut`.
- A field is mutable when its base place is mutable.
- A `mut` receiver or parameter is a mutable place inside its body.

Parameter bindings themselves are not reassigned.

Shared-borrow and `own` parameters can have defaults. An omitted shared default creates a fresh temporary that lives through the call. An omitted owned default creates a fresh value that the call consumes. A `mut` parameter cannot have a default, even for a copy type. The caller could not see its temporary, so every mutation would be a silent lost write. Require the caller to pass a mutable value, or take `own T` and return the result.

## Local Views And Place Identity

`view` creates a non-owning alias to one addressable place. The supported places are:

- local roots, parameters, receivers, and existing views
- class-field paths and fixed tuple positions
- list elements and dictionary entries, including a field or tuple projection inside an element, such as `users[i].name`

Set elements, Queue receives, Range values, and computed temporaries do not have view identity in Aura 0.3.

A source is evaluated once. An element or entry selector is read when the view is created, and its bounds or presence is checked there with `AU4003`. The view keeps naming that slot afterwards.

    class Counter:
        value: int64

    def main():
        mut counter = Counter(value=1)
        view mut value = counter.value
        view mut nested = value
        nested = nested + 1
        print(counter.value)

The mutable assignment writes immediately to `counter.value`. Ending the loan does not perform a delayed copy-back.

Views follow these access and overlap rules:

- A shared view permits reads.
- A mutable view is exclusive. It blocks every overlapping source access except through itself or a contained reborrow.
- Ancestors overlap descendants. Distinct fixed fields and tuple positions are disjoint.
- Two element or entry views of one collection are disjoint only when both selectors are literals of different values. A selector read from a variable overlaps every element.
- A structural mutation of the collection overlaps every element view.

Literal selectors with different values give disjoint element views:

    mut users = [Profile(name="ada", visits=1), Profile(name="linus", visits=2)]
    view mut ada = users[0]
    view mut linus = users[1]
    ada.visits += 1
    linus.visits += 1

A view binding cannot be rebound, moved, cloned as a descriptor, stored in an aggregate, or sent across a task or Queue boundary.

A list element or dictionary entry can be used in place wherever a place is borrowed. Aura lends it for the statement through a hidden element loan, as if you had written a `view`:

- a shared or `mut` argument, such as `show(names[0])` or `bump(values[i])`
- a method receiver, such as `users[0].describe()` or `counters[i].add(3)`
- an operand, such as `names[0] == "ada"` or `f"{names[0]}"`
- a `match` scrutinee, including `match mut messages[0]:`
- a nested selection, such as `bump(grid[i][j])`

Writes through a `mut` argument, a mutating receiver, or `match mut` land in the collection. The same overlap rules as for views apply: two literal positions or keys are disjoint, and a computed index overlaps every element, so `adjust(values[i], values[j])` with two `mut` parameters is refused.

Moving the element out is still refused (`AU3005`). This covers binding it to a new name, passing it to an `own` parameter, putting it in a new aggregate, and moving a non-Copy field out of it. Use `.clone()` for an explicit copy, or `pop`, `set`, or `remove` to transfer ownership. A shared read of an element whose type cannot be cloned, such as a `random.Rng`, also needs an explicit `view`.

A loan region begins at view creation and ends after the final possible use. The checker computes it conservatively across branches and loops, and the lexical scope is only an upper bound. While the loan is live, the checker rejects rebinding, moving, cleaning up, or structurally mutating an overlapping source. Scope exits, `return`, `break`, `continue`, propagated errors, traps, and cancellation release every loan they leave, in reverse acquisition order.

## Call-Boundary Exclusivity

All receiver and argument accesses for one call are checked together. Shared borrows can overlap other shared borrows. Every mutable borrow and every move must be exclusive with respect to any overlapping place.

```aura
class Acc:
    value: int32

    def add_from(mut self, source: Acc):
        self.value += source.value

def main():
    mut acc = Acc(value=1)
    # acc.add_from(acc) is rejected: mutable self overlaps shared source
```

Place overlap is prefix-based for tracked name and field paths:

- `value` overlaps `value.field`, and `value.field` overlaps `value.field.inner`.
- Distinct roots do not overlap.
- Sibling fields such as `pair.left` and `pair.right` are distinct when the checker can prove those paths.

The same exclusivity rule applies when one argument consumes a value and another argument borrows it. Argument evaluation order does not make an otherwise overlapping call legal.

## Partial Moves And Reinitialization

Moving a non-copy field out of an owned class marks that field path as moved. Disjoint fields stay usable:

```aura
class User:
    name: str
    id: int32

def main():
    mut user = User(name="Ada", id=1)
    name = user.name
    print(user.id)

    user.name = "Grace"
    print(user.name)
```

The complete class value cannot be used while any field remains moved. Assigning the exact moved field reinitializes that path. Assigning a fully moved mutable binding reinitializes the binding and clears its moved-field state.

A borrower does not own the containing value, so moving a non-copy field through a shared or mutable borrow is rejected:

```aura
def bad(user: User) -> str:
    return user.name # rejected
```

Use `.clone()` for a new owned value when the type supports it, or expose an owner method that performs the read or mutation:

```aura
def good(user: User) -> str:
    return user.name.clone()
```

## Flow-Sensitive Move Checking

Branches and match arms are checked independently. At a reachable join, a binding or field counts as moved if any incoming path may have moved it. The exception is a place that every relevant path definitely reinitialized.

Loops add one more condition, because a loop may run again. Aura rejects a first move or partial move from an outer value inside a repeatable loop when the next iteration could reuse the moved place.

Limited constant-boolean reasoning recognizes forms built from `true`, `false`, grouping, and `not`. Programs should not depend on broader compile-time evaluation.

Block-local bindings do not escape their branch, arm, loop, or `with` body. See [Names And Scopes](/manual/names-and-scopes#block-scope-and-control-flow).

## Owned Returns

Every ordinary `-> T` function return transfers an owned value to its caller. Copy values are ordinary independent copies. A non-copy return must come from an owned source:

```aura
def identity(value: int32) -> int32:
    return value

class User:
    name: str

def copy_name(user: User) -> str:
    return user.name.clone()

def into_name(user: own User) -> str:
    return user.name
```

A function gets an owned non-copy result in one of four ways:

- construct a fresh value
- clone a clone-safe value
- accept an `own` parameter
- consume an owner through an `own self` method

Shared or mutable access does not transfer ownership of a non-copy field. Returning that field directly is rejected as an invalid move through access the function does not own. [Functions](/manual/functions#owned-returns) has the detailed rules.

## Returned Views

A declaration can instead return a view tied to exactly one receiver or ordinary parameter:

    class User:
        name: str

    class Counter:
        value: int64

    def name(user: User) -> view str from user:
        return view user.name

A mutable returned view needs a `mut` origin. This continuation defines one and uses both views:

    def value_mut(counter: mut Counter) -> view mut int64 from counter:
        return view mut counter.value

    def bump(value: mut int64):
        value += 1

    def main():
        user = User(name="Ada")
        view current = name(user)
        print(current)
        print(name(user))

        mut counter = Counter(value=0)
        view mut editable = value_mut(counter)
        editable += 1
        print(counter.value)
        bump(value_mut(counter))
        print(counter.value)

The origin is part of the callable contract:

- A shared result can originate from bare or mutable access. A mutable result requires a `mut` origin.
- An `own` or defaulted parameter, a callee local, a temporary, or a newly allocated value cannot be an origin.
- A caller must supply an addressable origin.
- Trait implementations must use the same receiver or parameter slot as the trait declaration, even if parameter names differ.

A caller can use the result in three ways:

- initialize a matching `view` or `view mut` binding
- read a shared result directly within one containing expression
- reborrow a mutable result immediately into a `mut` call

Ordinary owned bindings and aggregate storage cannot receive a returned view.

A returned view names a root, field path, or fixed tuple position of its origin. It cannot select a list element or dictionary entry: `return view items[0]` is refused with `AU3004`. Return a view of the collection and select the element at the call site.

Different return paths can select different fixed projections of the declared origin. The caller locks that origin conservatively, while execution keeps the exact selected projection. A different root is `AU3010`.

Ordinary `-> T` stays an owned return. Structural `def(...) -> R` types do not erase a returned view's origin.

## Borrowed Pattern Matching

The match form sets the mode for an enum scrutinee:

- `match own` consumes a non-copy enum scrutinee.
- Bare `match` retains the enum and gives non-copy payload bindings shared-borrow provenance.
- `match mut` requires a mutable place and gives non-copy payload bindings mutable borrows.

```aura
result: Result[str, str] = Result.Ok("ready")

match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(error):
        print(error)
```

Under `match mut`, mutations are written back by reconstructing the enum on normal arm exit, `return`, `break`, `continue`, and `try` propagation. A nested mutable match cannot overlap an already active mutable match. Reassigning the exact scrutinee, its root, or an ancestor field invalidates payload bindings tied to the old value. A write to a proven-disjoint sibling field does not invalidate them.

Tuple patterns follow a smaller rule:

- `match own` on a tuple consumes the whole non-copy scrutinee and gives owned leaf bindings.
- Bare `match` retains the tuple and gives shared leaf provenance.
- Tuple patterns are rejected under `match mut`. Aura does not reconstruct and write back recursive tuple targets.

Payload bindings are arm-local and cannot shadow a visible binding. [Enums And Pattern Matching](/manual/enums-and-match) specifies match typing and exhaustiveness.

## Borrowed Iteration

The loop form sets the mode for `list` and `set` iteration:

- Bare iteration retains the collection and yields shared-borrowed non-copy elements. The selected place is frozen against overlapping mutation for the loop body.
- `for value in own collection` moves the collection once into a loop-private source and yields owned elements. Reinitializing the consumed source binding in the body cannot switch or truncate the active iteration.
- `for value in mut values` requires a mutable list place and yields mutable-borrowed elements.

Mutable-borrow set iteration is not supported. Mutate a set through `add` and `remove` outside borrowed iteration.

Queue iteration receives values. It is a scheduler operation, not a place traversal. The bare form copies the Queue handle once at loop entry and yields owned items without freezing the source binding. Rebinding that source does not switch later receives. All three explicit ownership modifiers are rejected. See [Concurrency](/manual/concurrency).

When an iteration item is a tuple, recursive target leaves inherit the item's provenance:

- Shared collection iteration gives shared non-copy leaves.
- `own` collection iteration gives owned leaves.
- Bare Queue iteration gives owned leaves, because it receives the item.
- A tuple target is rejected with `mut` iteration. Recursive mutable tuple writeback is not defined.

## Clone

`.clone()` explicitly creates another owned structural value where the maintained type exposes cloning:

```aura
name = "aura"
copy = name.clone()
print(name)
print(copy)
```

Text clones and collection copies create owned contents. Cloning a runtime-backed resource handle does not necessarily create an independent host resource. Rely on the resource's documented API.

Not every move type supports cloning. `random.Rng` is deliberately non-cloneable. Wrapping it in a class, enum, or collection does not make the stored generator cloneable. Clone-producing collection reads and task observations follow the same structural rule. Copying a `Task[T]` or `Queue[T]` handle is different, because it copies only the handle, not a stored `T`.

When a clone-producing operation depends on an unresolved generic type, the callable acquires an inferred clone-safety obligation. Safe specializations stay valid. A specialization that would duplicate `random.Rng` is rejected with `AU3007`. See [Generics And Traits](/manual/generics-and-traits#inferred-clone-safety-obligations).

## Closures And Capture

A lambda without a capture list captures by value when it is created:

- A referenced outer Copy value is copied into the closure environment.
- A referenced outer non-Copy owned value is moved. The source cannot be used afterward unless the program cloned it before creating the closure.

A read-only closure borrows its environment for each call and is repeatable, even when it owns non-Copy data. A closure whose body consumes any non-Copy capture is itself consumed by the call. It is single-use under `AU3001`.

Capturing closures are non-Copy. Their environment is `Transfer` only when every captured value is `Transfer`. `Transfer` is the property a value needs to cross a task or Queue boundary. See [Tasks And Borrowing](#tasks-and-borrowing).

An explicit exhaustive capture list requests live capabilities:

    read = lambda [settings] key: settings.lookup(key)
    mut update = lambda [mut stats] value: stats.record(value)
    snapshot = lambda [own cache] key: cache.get(key)

- A bare entry creates a shared loan.
- A `mut` entry creates an exclusive mutable loan.
- An `own` entry uses the copy-or-move capture described above.

Every used outer local must be listed exactly once, and every listed local must be used. A mutable-loan closure is mutable-repeatable and must be called through a mutable closure place. A loan closure is non-Copy, non-Transfer, synchronous, and local. See [Closures](/manual/closures).

## FFI Views And Opaque Handles

FFI is the foreign function interface. FFI v0 views are temporary call-boundary capabilities, not first-class Aura references.

- Bare `str` and `list[uint8]` retain their owner while exposing a const pointer and byte length for one synchronous foreign call.
- `mut list[uint8]` requires an exclusive mutable list place. It copies the initial bytes into a same-length scratch buffer, and writes exactly that length back after the foreign function returns.
- Empty views use a null pointer with length zero.
- Foreign code must not retain any view pointer.

An opaque FFI handle is a non-Copy, non-cloneable, non-Transfer owned wrapper for one non-null foreign pointer. A bare handle parameter retains it. `own Handle` consumes it. `mut Handle` is unavailable.

Aura does not call a foreign destructor automatically, so a binding must invoke its explicit consuming close or free declaration. Opaque handles cannot be task captures, task results, or Queue payloads.

See [FFI v0](/manual/ffi) for the complete ABI and failure boundary.

## Tasks And Borrowing

The four `TaskGroup` start methods accept named functions or associated methods with bare shared or `own` parameters. `mut` targets are rejected. The two `_with_stack` forms add an `int64` capacity argument before the callable. They do not change capture ownership.

```aura
def worker(label: str):
    print(label)

with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label.clone())
    print(label)
```

Each task argument is copied or moved into task-owned capture storage before the child runs. The target then shares or consumes that capture according to its declared mode. Copied task and queue handles still refer to shared runtime state. See [Concurrency](/manual/concurrency) and [Execution Model](/manual/execution-model#tasks-and-scheduler).

A separate `Transfer` check applies to task captures, task results, Queue construction, and Queue `put` and `try_put`. Handle-only Queue operations do not recheck the payload. A bare target parameter can still borrow its child-owned capture for the call, but the captured value itself must be transferable. A shared or mutable capability view cannot cross the boundary, so pass owned structural data instead.

| Category | Types |
| --- | --- |
| Qualifies | Copy values, `str`, and aggregates made entirely from transferable components. `process.Completed`, `net.HttpResponse`, and `net.UdpDatagram`, which are owned snapshot data rather than live resources. |
| Does not qualify | `random.Rng`, `TaskGroup`, and live host resources, including `process.Child`, `net.HttpExchange`, and `net.UdpSocket`. |

A Copy value read through shared or mutable access is a narrow exception. Task capture materializes an independent owned snapshot, so no capability crosses. A non-copy access cannot be captured this way, because the child would need ownership of the value.

The same decision divides task results statically into repeatable values and single-consumer values:

- `Task[T]` is copyable only when `T` is copyable, a `Queue[...]`, or a recursively repeatable `Task[...]`.
- Otherwise each result method consumes the unique observation right, even when it reports timeout, cancellation, or failure.
- Multi-task waits consume their entire task list, and `wait_any` abandons the unchosen rights.

The pinned-worker runtime relies on these rules to run sibling task bodies safely on different host threads. Queue and Task handle identity can cross workers. Every other capture or result stays an owned `Transfer` value rather than a shared capability.

## Resources And `with`

Resource ownership should normally be lexical:

```aura
import fs
import io

def show_file() -> Result[None, io.Error]:
    with file = try fs.open("data.txt"):
        text = try file.read_all()
        print(text)
    return Result.Ok(None)
```

`with` consumes the resource expression and creates a fresh mutable managed binding. A managed resource or its non-copy fields cannot be moved out in a way that would prevent cleanup. The registered `close` runs on normal fallthrough, `return`, escaping loop control, `try` propagation, and maintained runtime failure. Nested cleanups run in reverse order.

Each builtin resource's module chapter defines its behavior. A user class resource must be non-generic and define `close(mut self) -> None` with no ordinary parameters. [Execution Model](/manual/execution-model#resource-lifetime-and-cleanup) specifies full cleanup ordering and failure precedence.

## Grammar

[Grammar](/manual/grammar) has the productions for the normative capability spellings:

- bare, `own`, and `mut` ordinary parameters
- `self`, `own self`, and `mut self` receivers
- bare, `own`, and `mut` collection loops, where the iterable supports them
- bare `match`, `match own`, and `match mut`
- mutable bindings
- local and returned `view` forms
- explicit lambda capture lists
- owned return annotations
- `with`

Call arguments themselves never carry a capability prefix.

## Typing Rules

Every expression has one static copy or move category. Every parameter has one declaration-stable passing mode:

- Bare parameters grant logical shared access for every type. An implementation may pass copy bits directly.
- Explicit `own` consumes.
- `mut` requires one exclusive mutable place.
- Shared and owned defaults are legal, and shared temporaries last through the call. `mut` defaults are rejected.

The checker verifies these before lowering: place-prefix overlap, partial moves, control-flow joins, loop repetition, owned-return moves, view provenance and last-use regions, borrowed matches, borrowed iteration, task capture, and managed-resource containment.

Clone-producing generic operations infer obligations. Calls propagate them, and specialization discharges them.

## Runtime Semantics

A copy use duplicates a value and a move transfers it. Ordinary parameter borrows stay call-scoped access contracts.

An explicit view carries a compiler/runtime loan descriptor for one source place and generation. Reads and writes resolve through that descriptor without cloning.

Mutable borrowed calls and list iteration write through the original place. `match mut` reconstructs and writes back on every arm exit.

Simple dict indexed assignment accepts and owns any value type. Direct compound indexed assignment requires a copy `list` element or `dict` value.

Task start first transfers captures into child-owned storage. `with` owns one cleanup registration and runs it exactly once on every maintained scope exit, under the documented failure-precedence rules.

## Ownership And Evaluation Order

Subexpressions evaluate in the order defined by [Execution
Model](/manual/execution-model#evaluation-order). Then a copy, move, or borrow applies at its typed boundary.

All receiver and argument accesses for one call are checked together, so source order cannot legalize overlapping shared, mutable, and owned uses. A partial move preserves proven-disjoint fields, and reinitializing the exact moved place restores it. Control-flow merging never silently restores ownership. Aura never inserts a clone or coercion to repair an invalid use.

Capturing a copy place duplicates its value. A non-copy place selected as a binary left operand, index base, method receiver, or indexed-assignment target stays borrowed until that operation consumes all of its inputs:

- A later shared borrow is permitted.
- An overlapping mutable borrow or consumption is rejected with `AU3002`. The diagnostic identifies the retained selection as the borrow origin.

Name roots and projected member places follow the same rule. No backend inserts a hidden deep clone. An operation that needs a point-in-time representation produces it immediately. Each f-string interpolation renders to `str` before the next interpolation begins.

Compound assignment uses the matching binary operator dispatch. That includes applicable user-defined operator traits for root and projected targets.

- A copy target is captured before the right operand.
- A non-copy root or projected target stays borrowed across that operand, so an overlapping mutable borrow or consumption is `AU3002`.
- A non-copy `list` element or `dict` value cannot be a direct compound target. Aura rejects the operation instead of cloning or destructively moving the stored value.

## Diagnostics

Ownership failures are static.

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed ownership, receiver, loop, match, or return syntax. |
| `AU2002` | Type mismatch. |
| `AU2004` | Argument binding that cannot satisfy a required mutable place. |
| `AU2999` | An unsupported move, control-flow, or resource case with no narrower category. |
| `AU3001` | Use of a moved or partially moved place, including reuse after a direct task-result observation. |
| `AU3002` | Overlapping or invalid borrows, moving through a borrow, invalid mutable-borrow defaults or task targets, stale borrowed-pattern bindings, consuming a task-result right through shared access, and later mutable or consuming access that overlaps a retained non-copy binary operand, index base, method receiver, or indexed-assignment target. |
| `AU3003` | Assignment or mutation through an immutable place, including shared `self`. |
| `AU3004` | Invalid parameter, receiver, loop, or Queue-iteration ownership modes. |
| `AU3005` | Moving a non-copy list element or dict value out of its collection, or a shared read of an element whose type cannot be cloned. |
| `AU3006` | The corresponding indexed compound read-modify-write. |
| `AU3007` | Direct or transitive duplication of non-cloneable state, including `random.Rng`, opaque FFI handles, capturing closure environments, and unsafe generic specializations. |
| `AU3008` | A non-Transfer task or Queue boundary. |
| `AU3009` | A clone, clone-producing collection read, or aggregate copy that would duplicate a single-consumer task-result right. |
| `AU3010` | An invalid view escape, returned-view origin, or provenance path. |

For `AU3005`, every borrowed use of an element is a place read, not a move.

For a retained-expression `AU3002`, the diagnostic points to both the later access and the retained-borrow origin.

View diagnostics identify the creation and final use that keep the conflicting loan alive. They recommend shortening the region, using a disjoint place, or producing an owned clone.

A runtime operation reached through an owned or borrowed value keeps its own code:

| Code | Cause |
| --- | --- |
| `AU4001` | General trap. |
| `AU4002` | Arithmetic overflow or underflow. |
| `AU4003` | Bounds or lookup violation. |
| `AU4004` | Zero divisor. |
| `AU4005` | Resource or I/O failure. |

## Backend Support

The compiler runs one ownership and borrow analysis before it selects a backend. MIR execution and direct native generation receive the same resolved parameter ABI, moves, copies, capture modes, borrowed-match and iteration operations, and cleanup registrations. MIR is the compiler's mid-level intermediate representation.

Analysis and language server signatures expose the same modes. The parity matrix pins observable move, mutation, capture, writeback, cleanup, and primary-diagnostic behavior.

## Limits And Implementation-Defined Behavior

Place analysis tracks local roots, fixed tuple positions, and field-prefix paths. It proves disjoint fixed projections, but it is not a general alias theorem.

These are unavailable:

- returned views of list elements or dictionary entries
- view-bearing aggregates
- multi-origin returned views
- returned loan closures
- lifetime-parameterized callable types
- mutable set iteration
- explicit Queue ownership modifiers
- mutable-borrow task targets
- moving out of a managed resource
- arbitrary reference values

Loop move analysis intentionally uses only the limited Boolean reasoning described in [Flow-Sensitive Move Checking](#flow-sensitive-move-checking).

Ownership mode and evaluation order are language-defined, not backend-defined or host-defined.

## Status

Implemented:

- copy and move classification
- declaration-stable parameter defaults
- explicit owned, shared, and mutable passing, and all receiver modes
- call-boundary exclusivity
- partial moves and reinitialization
- flow-sensitive checks
- owned returns
- borrowed matching and borrowed `list` and `set` iteration
- the one-time `list`, `set`, and Queue iteration-source rule
- task capture
- cloning
- lexical resource ownership
- place-based local and returned views, inferred regions, and reborrowing
- explicit loan closure captures
- unified loan cleanup

Mutable set iteration, Queue ownership modifiers, and mutable task capture are unavailable.

## Design Record

- [ADR-0017: Iteration source selection](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0017-iteration-source-selection.md)
- [ADR-0033: Structural Transfer and task-result consumption](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md)
- [ADR-0037: Expression closures and value capture](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0037-expression-closures-and-value-capture.md)
- [ADR-0038: Place-based loans and views](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0038-place-based-loans-and-views.md)
