# Values, Moves, And Borrows

This is the central chapter of the book. Almost everything in Aurora — how functions receive data, how collections hold it, how tasks share it, how resources get cleaned up — follows from the rules introduced here.

The short version:

- Every value has an owner.
- Moving a value transfers ownership.
- Borrowing lets another piece of code use a value without taking it.
- Mutable borrows are exclusive.
- Resources should live inside a `with` block.

Read the rest of the chapter to see why each of those matters.

## Copy Values And Move Values

Some values are cheap enough to duplicate that the language just does it. Numbers, `bool`, `Duration`, queue handles, and task handles are **copy types**. Assigning one to a new name produces another usable binding:

```python
count = 3
other = count

print(count)
print(other)
```

Everything else — `String`, `Vec[T]`, `Map[K, V]`, `Set[T]`, class instances, `TaskGroup`, file handles, process and network resources — is a **move type**. Assigning a move value transfers ownership:

```python
name = "aurora"
other = name

# name has moved into other. Using name is a compile error.
print(other)
```

The rule prevents two bindings from thinking they are responsible for the same owned resource. It is the reason a string, a file handle, and a task group can all be closed automatically when their owner goes out of scope.

## Cloning When Two Owners Are Needed

If a program genuinely wants two independent copies of a move value, it says so with `.clone()`:

```python
name = "aurora"
copy = name.clone()

print(name)
print(copy)
```

Collections clone their elements when you clone the collection:

```python
jobs = ["parse", "check", "build"]
snapshot = jobs.clone()

print(jobs.len())
print(snapshot.len())
```

Clone close to the reason for cloning. A clone at the call site tells the reader that the program is deliberately keeping both values.

## Shared Borrows

When a helper should read a value without owning it, the parameter uses `borrow T`:

```python
def render_title(title: borrow String) -> String:
    return title.to_upper()

title = "manual"
print(render_title(title))
print(title)
```

The call site does not write `borrow`; Aurora reads the borrow form from the function signature. The caller keeps ownership, and the helper cannot move a non-copy value out of the borrowed view.

Classes make the benefit obvious:

```python
class Job:
    id: int32
    label: String

def render(job: borrow Job) -> String:
    return f"{job.id}: {job.label}"

job = Job(id=7, label="compile")
print(render(job))
print(render(job))
```

The same job is rendered twice because `render` never takes ownership.

## Mutable Borrows

When a helper should mutate a caller-owned value, the parameter uses `borrow mut T`:

```python
def add_job(jobs: borrow mut Vec[String], job: own String):
    jobs.push(job)

mut jobs = Vec[String]()
add_job(jobs, "parse")
add_job(jobs, "check")
print(jobs.len())
```

Two rules apply to mutable borrows:

1. The caller's binding must itself be mutable. You cannot take `borrow mut` from an immutable binding or a temporary value.
2. A mutable borrow is **exclusive**. If one argument to a call takes `borrow mut`, no other argument in that call may borrow the same value. This is not a stylistic preference; overlapping mutable aliases would make the order of effects unclear, and Aurora rejects them at the call boundary rather than relying on the callee to behave well.

## Methods And `self`

Methods declare how they receive `self`, and the receiver form determines what the method is allowed to do:

```python
class Counter:
    value: int32

    def get(self) -> int32:
        return self.value

    def inc(borrow mut self):
        self.value += 1
```

Bare `self` reads through a shared borrow; `borrow self` is its explicit
synonym. `borrow mut self` writes. A consuming method uses `own self` and takes
ownership of the whole instance.

A borrowed method may look at non-copy fields but cannot move them out:

```python
class Label:
    text: String

    def show(self) -> String:
        return self.text.clone()
```

`self.text.clone()` returns a new owned `String` to the caller. Returning `self.text` without cloning would try to move a `String` out through a shared borrow, which the compiler rejects.

## Field Moves

Owned fields are independent. A program can move one field out of a class without giving up the rest — but the moved field becomes unusable until it is reassigned:

```python
class Packet:
    id: int32
    body: String

mut packet = Packet(id=1, body="hello")
body = packet.body

print(packet.id)
packet.body = "replacement"
print(packet.body)
```

`packet.id` is still available because it was not moved. `packet.body` became uninitialised after the first move and could only be used again once it was reassigned. This is the same rule as for top-level bindings, applied field by field.

## Collections And Ownership

Collection operations that store values declare explicit `own` positions. For
example, `Vec.push(value: own T)`, `Map.set(key: own K, value: own V)`, and
`Set.insert(value: own T)` move non-copy values into their collection. If the
caller still needs one, clone it.

```python
mut jobs = Vec[String]()
label = "compile"
jobs.push(label.clone())
print(label)
```

Lookup methods such as `Vec.get` and `Map.get` return cloned owned values. The collection keeps its element, and the caller receives an independent copy:

```python
names = ["ada", "grace"]

match names.get(0):
    case Some(name):
        print(name)
    case None:
        print("missing")
```

This is why a program can read from a collection repeatedly without juggling its ownership.

## Tasks And Borrowing

Child tasks receive **owned captures**. The start operation moves or copies each
argument into task-owned storage before the child can outlive the caller. The
target function can then borrow that capture or consume it:

```python
def worker(label: borrow String):
    print(label)

with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label)
```

The capture itself is owned by the task, so starting it still moves the
caller's non-copy value. If the parent also needs the label, clone before
starting the child:

```python
with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label.clone())
    print(label)
```

`TaskGroup` itself is a resource. Normal practice is to keep it scoped with `with`, so that leaving the block waits for the children and accounts for their results.

Default-mode and explicit shared target parameters borrow their task-owned
capture; `own` targets consume it. `borrow mut` targets are rejected because
mutation of detached capture storage would have no caller-visible writeback.

## Resources And Cleanup

Owned resources — files, listeners, streams, processes, supervisors, task groups — should live inside `with` blocks:

```python
import fs

with file = try fs.open("data.txt"):
    text = try file.read_all()
    print(text)
```

When the block exits, Aurora runs the resource's cleanup path. Cleanup fires on normal exit **and** on runtime errors that unwind through the scope, in both `aura run` and built programs. `with` is the place where "I borrowed a resource" becomes "the resource has definitely been released."

## A Checklist

When a program starts to feel tangled, run down this list:

- Write `own T` when the function consumes the argument; a bare non-copy
  parameter borrows by default.
- Pass `borrow T` when the function only needs to inspect.
- Pass `borrow mut T` when the function should update a caller-owned value.
- Clone as locally as possible when two owners are genuinely needed.
- Put resources in `with` blocks.
- Put concurrent child work inside a `TaskGroup`.
- Let `Result`, `Option`, and the outcome enums carry control flow — don't smuggle failure through strings or magic values.

The goal is not to fight the checker. The goal is to make the program say who is responsible for every value.

Reference: [Ownership And Borrowing](/manual/ownership-and-borrowing).
