# Values, Moves, And Borrows

This is the central chapter of the book. Most of Aura follows from the rules here: how functions receive data, how collections hold it, how tasks share it, and how resources get cleaned up.

The short version:

- Every value has an owner.
- Moving a value transfers ownership.
- Borrowing lets other code use a value without taking it.
- Mutable borrows are exclusive.
- Resources should live inside a `with` block.

The rest of the chapter shows why each rule matters.

## Copy Values And Move Values

A **copy type** is cheap to duplicate, so Aura duplicates it on assignment. Numbers, `bool`, `Duration`, and queue handles are copy types. Assigning one to a new name leaves both bindings usable:

```aura
count = 3
other = count

print(count)
print(other)
```

Every other type is a **move type**. That includes `str`, `list[T]`, `dict[K, V]`, `set[T]`, `random.Rng`, ordinary class instances, `TaskGroup`, file handles, process resources, and network resources. Assigning a move value transfers ownership:

```aura
name = "aura"
other = name

# name has moved into other. Using name is a compile error.
print(other)
```

The rule stops two bindings from both claiming the same owned value. Because each value has one owner, Aura can close a string, a file handle, or a task group automatically when its owner goes out of scope.

Task handles depend on their result type. `Task[T]` is copyable when `T` is copyable, a `Queue[...]` handle, or a recursively repeatable `Task[...]` handle. A task that returns `str`, `list[...]`, or another non-copy owned value has a move-only handle. That way, aliases cannot duplicate its single right to observe the result.

## Cloning When Two Owners Are Needed

When a move type supports independent duplication, ask for it with `.clone()`:

```aura
name = "aura"
copy = name.clone()

print(name)
print(copy)
```

A collection's `copy()` creates independent storage and clones each element:

```aura
jobs = ["parse", "check", "build"]
snapshot = jobs.copy()

print(jobs.len())
print(snapshot.len())
```

Every element must be clone-safe for this to work. `random.Rng` has no clone route by design. Putting one inside a list, dictionary, class, or enum does not change that. A generic clone helper stays valid: Aura infers the requirement and rejects only a specialization that would duplicate an `Rng`.

Clone close to the reason for cloning. An explicit `clone()` or `copy()` at the call site tells the reader that the program keeps both values on purpose.

## Closures Own Their Captures

A closure takes its captured values when the lambda expression runs:

```aura
label = "compile"
length: def() -> int64 = lambda: label.len()

print(length())
print(length())
```

`label` is not a copy type, so it moves into `length`. You can still call the closure many times because its body only reads the captured string. If the body returned `label` itself, the first call would consume the capture and with it the whole closure. A second call would then be a moved-value error.

To keep both owners, clone before you create the closure:

```aura
label = "compile"
captured = label.clone()
length: def() -> int64 = lambda: captured.len()

print(label)
print(length())
```

- A copy capture is a snapshot and leaves the source usable.
- Shared and mutable enclosing parameters are capabilities, so a closure cannot capture them as owned values.
- Captured environments are read-only.

## Shared Borrows

When a helper only reads a value, declare the parameter as plain `T`:

```aura
def render_title(title: str) -> str:
    return title.to_upper()

title = "manual"
print(render_title(title))
print(title)
```

The call site writes no prefix. Aura takes the shared form from the function signature. The caller keeps ownership, and the helper cannot move a non-copy value out through shared access.

The benefit is clearer with a class:

```aura
class Job:
    id: int32
    label: str

def render(job: Job) -> str:
    return f"{job.id}: {job.label}"

job = Job(id=7, label="compile")
print(render(job))
print(render(job))
```

`render` never takes ownership, so the same job renders twice.

## Mutable Borrows

When a helper changes a value its caller owns, declare the parameter as `mut T`:

```aura
def add_job(jobs: mut list[str], job: own str):
    jobs.append(job)

mut jobs = list[str]()
add_job(jobs, "parse")
add_job(jobs, "check")
print(jobs.len())
```

Two rules apply:

1. **The caller's binding must be mutable.** You cannot take `mut` access from an immutable binding or a temporary value.
2. **Mutable access is exclusive.** If one argument to a call takes `mut`, no other argument in that call may borrow the same value. Overlapping mutable aliases would make the order of effects unclear, so Aura rejects them at the call boundary.

## Views Of Elements

A view borrows one element, entry, or field in place, without cloning or moving it. `view name = place` creates a shared alias. `view mut name = place` creates a mutable alias that writes through to the source.

```aura
class Profile:
    name: str
    visits: int32

mut users = [Profile(name="ada", visits=1)]

view name = users[0].name
print(name.len())

view mut visits = users[0].visits
visits += 1
print(users[0].visits)

mut counts: dict[str, int32] = {"parse": 1}
view mut count = counts["parse"]
count += 1
print(counts["parse"])
```

This prints `3`, `2`, and `2`.

- The index or key is evaluated once, when the view is created. A position outside the list or an absent key fails there with `AU4003`.
- While a view is live, the checker rejects an overlapping mutation, move, rebind, cleanup, or other mutable loan.
- Views of different literal positions or keys of one collection do not overlap. A computed index or key overlaps every element.
- A structural change to the collection, such as `append`, `remove`, or `clear`, overlaps every element view.
- A view's loan ends after its last use.

See [View Bindings](/manual/statements#view-bindings) for the full rules.

## Methods And `self`

A method declares how it receives `self`. The receiver form decides what the method may do:

```aura
class Counter:
    value: int32

    def get(self) -> int32:
        return self.value

    def inc(mut self):
        self.value += 1
```

| Receiver | Access |
| --- | --- |
| `self` | Reads through a shared borrow. `self` is also its explicit synonym. |
| `mut self` | Writes to the instance. |
| `own self` | Consumes the method's instance and takes ownership of all of it. |

A borrowed method may look at non-copy fields but cannot move them out:

```aura
class Label:
    text: str

    def show(self) -> str:
        return self.text.clone()
```

`self.text.clone()` returns a new owned `str` to the caller. Returning `self.text` without the clone would move a `str` out through a shared borrow, and the compiler rejects that.

## Field Moves

Each owned field has its own ownership. A program can move one field out of a class and keep the rest. The moved field is unusable until you assign it again:

```aura
class Packet:
    id: int32
    body: str

mut packet = Packet(id=1, body="hello")
body = packet.body

print(packet.id)
packet.body = "replacement"
print(packet.body)
```

`packet.id` stays available because it never moved. `packet.body` is uninitialized after the move and becomes usable again once it is reassigned. This is the rule for top-level bindings, applied field by field.

## Collections And Ownership

Operations that store a value take it as `own`. For example, `list.append(value: own T)`, dictionary indexed assignment, and `set.add(value: own T)` move non-copy values into the collection. If the caller still needs the value, clone it:

```aura
mut jobs = list[str]()
label = "compile"
jobs.append(label.clone())
print(label)
```

Lookup methods such as `list.get` and `dict.get` return a cloned owned value inside `Lookup[T]`. The collection keeps its element, and the caller gets an independent copy:

```aura
names = ["ada", "grace"]

match names.get(0):
    case Lookup.Found(name):
        print(name)
    case Lookup.Missing:
        print("missing")
```

So a program can read clone-safe values from a collection again and again without juggling ownership. A value that contains `random.Rng` must leave through an operation that transfers ownership instead, such as `list.pop`, `dict.remove`, or a Queue receive.

## Tasks And Borrowing

A child task receives **owned captures**. Starting the task moves or copies each argument into task-owned storage before the child can outlive the caller. The target function can then borrow that capture or consume it:

```aura
def worker(label: str):
    print(label)

with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label)
```

The task owns the capture, so starting it still moves the caller's non-copy value. If the parent also needs the label, clone it before starting the child:

```aura
with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label.clone())
    print(label)
```

`TaskGroup` is itself a resource. Keep it scoped with `with`. Leaving the block then waits for the children and accounts for their results.

The target function's parameters decide what happens to each capture:

| Target parameter | Effect |
| --- | --- |
| Plain `T` | Borrows the task-owned capture. |
| `own T` | Consumes the capture. |
| `mut T` | Rejected. Mutating detached capture storage would never write back to the caller. |

### The `Transfer` Rule

Ownership alone is not enough to cross a task boundary. Every capture and result must also be structurally `Transfer`.

- **Can cross:** copy data, `str`, recursively transferable collections and user data, and Queue and Task handle identities.
- **Cannot cross:** `random.Rng`, `TaskGroup`, capability views, and live file, process, or network resources.

Keep a live resource on the task that creates it. Exchange owned descriptions, bytes, snapshot results, or handles instead.

This rule is also the share-nothing boundary between pinned scheduler workers. Queue and Task handle state is synchronized across workers. Every other capture and result crosses as owned `Transfer` data.

A task result may be transferable but not repeatable. For such a result, the first call to `result`, `poll`, or `result_or` consumes the task handle. This holds even if the call times out, is cancelled, fails, or returns a fallback. Use a Queue protocol when several consumers each need their own message.

## Resources And Cleanup

Owned resources belong inside `with` blocks. These include files, listeners, streams, processes, supervisors, and task groups.

```aura
import fs

with file = try fs.open("data.txt"):
    text = try file.read_all()
    print(text)
```

When the block exits, Aura runs the resource's cleanup path. Cleanup runs on normal exit **and** when a runtime error unwinds through the scope. This holds in both `aura run` and built programs. A `with` block is where "I borrowed a resource" becomes "the resource is released".

## A Checklist

When a program starts to feel tangled, run down this list:

- Write `own T` when the function consumes the argument. A plain parameter grants shared access.
- Pass `T` when the function only needs to inspect.
- Pass `mut T` when the function should update a caller-owned value.
- Clone as locally as possible when you need two owners and the value is clone-safe.
- Put resources in `with` blocks.
- Put concurrent child work inside a `TaskGroup`.
- Let `Result`, `T | None`, `Lookup`, and the outcome enums carry control flow. Do not hide failure in strings or magic values.

The goal is not to fight the checker. The goal is a program that says who is responsible for every value.

Reference: [Ownership And Borrowing](/manual/ownership-and-borrowing).
