# Ownership And Borrowing

If you are coming from Python, this is the most important chapter in the
tutorial. Aura has no garbage collector. It tracks who owns each value and
when that value can be freed. This tracking is called ownership. Lending a
value for a while without giving it away is called borrowing.

The chapter builds the model one step at a time:

1. Why ownership exists, and which types copy or move.
2. How to clone, pass, and borrow values.
3. How methods, fields, loops, and `match` use the same rules.
4. Closures, views, and concurrency.
5. Fixes for the common compiler errors.

## Why Ownership?

Every value has exactly one owner at any time. When the owner goes out of
scope, Aura frees the value immediately. There is no garbage collector and no
reference counting.

Python works differently. Every value lives on a heap, and a garbage collector
frees it once nothing refers to it. That model is simple, but it brings
unpredictable pauses, higher memory use, and no deterministic cleanup.

Ownership gives you:

- **Predictable performance.** There are no garbage collection pauses.
- **Deterministic cleanup.** Resources such as files and connections close at
  a known point.
- **Memory safety.** The compiler rejects programs that would read freed or
  invalid memory.

The trade-off is that you need to think about who owns what. The compiler
enforces the rules and tells you when a program breaks one.

## Copy Types vs Move Types

Every Aura type is either a copy type or a move type. The rest of this chapter
builds on that split.

### Copy types

A copy type is a small, fixed-size value that is cheap to duplicate.
Assigning it to a new binding or passing it to a function makes a copy. The
original and the copy are independent.

The built-in copy types are:

- all integer types: `int` (the `int64` alias), `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`
- all unsigned types: `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`
- `float32`, `float64`
- `bool`
- `Duration`

Copy types behave the way Python developers expect:

```aura check-pass
x: int32 = 10
y = x          # copies the value
print(x)       # 10 -- still usable
print(y)       # 10 -- independent copy
```

Both `x` and `y` stay usable because `int32` is a copy type.

### Move types

A move type owns heap-allocated data or manages a unique resource. Assigning
it to a new binding moves ownership. The original binding becomes invalid.

The built-in move types include:

- `str`
- `list[T]`, `dict[K, V]`, `set[T]`
- `random.Rng`
- `TaskGroup`
- user-defined classes (by default)

Queue and task handles follow their own rules:

- `Queue[T]` is a copy handle to shared runtime state.
- `Task[T]` is always safe to transfer between tasks. It is copyable only when
  its result can be observed repeatedly. That means `T` must be copyable, a
  `Queue[...]` handle, or a recursively repeatable `Task[...]` handle.
- A task that returns `str`, `list[...]`, or another non-copy owned value
  therefore has a move-only handle.
- Copying an allowed handle never copies a queued value or task result.

Here is where Python intuition breaks down:

```aura check-pass
def main():
    name: str = "aura"
    other = name          # ownership moves to `other`
    print(other)          # "aura" -- works fine
```

This version uses `name` after the move:

```aura check-fail:AU3001
def main():
    name: str = "aura"
    other = name
    print(other)
    print(name)           # COMPILE ERROR
```

The compiler rejects it with:

```

error: use of moved value `name`
```

**Why does this happen?** After `other = name`, the `other` binding owns the
string data. If `name` stayed valid, two bindings would point to the same heap
memory. When both went out of scope, the memory would be freed twice, which
crashes the program. Aura catches this at compile time.

### The Python comparison

| Python | Aura |
|--------|--------|
| `y = x` always creates a reference, both point to the same object | `y = x` copies for copy types, moves for move types |
| Garbage collector handles cleanup | Owner handles cleanup when it goes out of scope |
| You never think about who owns what | You always know who owns what |

## Cloning: Explicit Copies Of Move Types

When a move type supports independent duplication, call `.clone()`:

```aura check-pass
name: str = "aura"
other = name.clone()   # explicit copy -- name stays valid
print(name)            # "aura"
print(other)           # "aura"
```

Collections use `copy()`:

```aura check-pass
def main():
    mut xs: list[int32] = [1, 2, 3]
    ys = xs.copy()         # independent copy
    xs.append(4)
    print(xs.len())        # 4
    print(ys.len())        # 3 -- unaffected
```

An explicit call makes the cost of allocating and copying elements visible.
Plain assignment still follows the copy-or-move rule.

Move types are not cloneable automatically. `random.Rng` has no clone
operation. A class, enum, or collection that contains one cannot be cloned
through a public clone-producing operation. Generic clone helpers infer this
requirement and reject an unsafe concrete specialization with `AU3007`.

List and `str` slices also produce an explicit owned copy:

```aura check-pass
names = ["Ada", "Grace", "Margaret"]
selected = names[1:]       # fresh owned list[str]
label = "A🎉Z"[1:2]       # fresh owned str containing 🎉
print(names.len())         # the sources remain valid
```

- A list slice copies copy elements and clones non-copy elements, so its
  element type must be clone-safe.
- A list slice rejects a value containing `random.Rng` with `AU3007`. It
  rejects a non-repeatable task result right with `AU3009`.
- A `str` slice copies its range of Unicode scalars.
- Neither slice is a view. Mutating the returned list cannot change the
  source, and a slice cannot be an assignment target.

## Passing Values To Functions

A bare parameter type grants shared access, for every type. To transfer a move
value into a function, write `own`:

```aura check-fail:AU3001
class Document:
    title: str
    pages: int32

def archive(doc: own Document):
    print(doc.title)

def main():
    doc = Document(title="Report", pages=42)
    archive(doc)
    print(doc.pages)       # COMPILE ERROR: use of moved value `doc`
```

The `own` parameter takes ownership of `doc`, so `doc` is invalid in `main`
after the call. With a bare `doc: Document`, the function would borrow and the
caller could keep using `doc`.

For a copy type, the compiler may pass the copied bits to implement shared
access. That does not change the contract:

```aura check-pass
def double(x: int32) -> int32:
    return x * 2

value: int32 = 5
print(double(value))   # 10
print(value)           # 5 -- still valid, it was copied
```

## Borrowing: Lending Without Giving Away

A borrow is a temporary loan. The function can access the value, and the
caller keeps ownership. Most functions that read or modify a value should
borrow it.

Aura has two kinds of borrow:

- `T`: shared, read-only access
- `mut T`: exclusive, mutable access

### Shared access with a bare type

A shared borrow lets a function read a value without consuming it:

```aura check-pass
class Counter:
    value: int32

def read(counter: Counter) -> int32:
    return counter.value

mut counter = Counter(value=41)
print(read(counter))       # 41
print(counter.value)       # 41 -- counter still belongs to us
```

The bare `counter: Counter` declaration says the function looks but does not
take. The borrow ends when the call returns, and the caller still owns the
value.

Several shared borrows can be active at once, because none of them can modify
the value:

```aura fragment
def sum_values(a: Counter, b: Counter) -> int32:
    return a.value + b.value

c1 = Counter(value=10)
c2 = Counter(value=20)
print(sum_values(c1, c2))   # 30 -- both still valid
```

### Mutable borrows with `mut T`

A mutable borrow lets a function modify the value in place:

```aura fragment
def bump(counter: mut Counter):
    counter.value += 1

mut counter = Counter(value=41)
bump(counter)
print(counter.value)       # 42 -- the change persisted
```

The caller must declare the binding `mut`, because the function will modify
it. The compiler rejects the call on an immutable binding:

```aura fragment
counter = Counter(value=41)  # not mutable
bump(counter)                # COMPILE ERROR
```

```

error: argument for parameter `counter` in function `bump` must be a mutable place
```

### The exclusivity rule

Mutable access cannot overlap any other access to the same value. This rule
prevents data races and aliasing bugs:

```aura fragment
def bad(a: mut Counter, b: Counter):
    a.value += b.value

mut c = Counter(value=1)
bad(c, c)    # COMPILE ERROR: overlapping access
```

**Why does this rule exist?** Here `a` and `b` are the same object. The
function increments `a.value` while it reads `b.value`, so the result would
depend on the order of operations inside the function. Aura rejects the call.

Think of a library book. Many people can read it at the same time, like shared
borrows. One person can take it home to annotate it, like a mutable borrow.
Both cannot happen at once.

## Method Receivers

Methods use the same borrowing rules through their receiver. The receiver
decides what the method can do with the instance.

### `self` -- read the instance

```aura check-pass
class Account:
    balance: float64

    def display(self) -> str:
        return f"Balance: {self.balance}"
```

Bare `self` is shared access. The method can read fields but cannot modify
them, and the caller keeps ownership:

```aura fragment
account = Account(balance=100.0)
print(account.display())    # "Balance: 100.0"
print(account.balance)      # still accessible
```

### `mut self` -- modify the instance

```aura check-pass
class Account:
    balance: float64

    def deposit(mut self, amount: float64):
        self.balance += amount

    def display(self) -> str:
        return f"Balance: {self.balance}"
```

The method can read and write fields. The instance must be declared `mut`:

```aura fragment
mut account = Account(balance=100.0)
account.deposit(50.0)
print(account.display())    # "Balance: 150.0"
```

Without `mut`, the call fails:

```aura fragment
account = Account(balance=100.0)
account.deposit(50.0)       # COMPILE ERROR: must be a mutable place
```

### `own self` -- consume the instance

```aura check-pass
class Connection:
    host: str

    def into_host(own self) -> str:
        return self.host
```

An `own self` receiver takes ownership. The call consumes a non-copy instance:

```aura fragment
conn = Connection(host="example.com")
host = conn.into_host()
print(host)               # "example.com"
print(conn.host)          # COMPILE ERROR: use of moved value `conn`
```

Use `own self` when the method takes the instance apart or transfers ownership
of its fields.

### No receiver -- associated methods

A method without a receiver is called on the class itself, not on an instance:

```aura check-pass
class Counter:
    value: int32

    def zero() -> Counter:
        return Counter(value=0)
```

```aura fragment
c = Counter.zero()
```

### Choosing the right receiver

| Receiver | When to use | Example |
|----------|-------------|---------|
| `self` | Read-only shared access, the default | getters, display, serialization |
| `mut self` | Modify the instance in place | setters, increment, append |
| `own self` | Consume the instance to extract data | `into_*` conversions, one-shot use |
| no receiver | Factory methods and utilities that do not need an instance | `Counter.zero()` |

If you are not sure, start with bare `self`. Add `own` only when the method
must consume the instance, or `mut` when it must mutate it in place.

## Field Access And Move Semantics

When you own a value, reading a non-copy field moves that field out of the
instance:

```aura check-fail:AU3001
class User:
    name: str
    age: int32

def main():
    user = User(name="Ada", age=36)
    greeting = user.name     # moves `name` out of `user`
    print(greeting)          # "Ada"
    print(user.age)          # 36 -- copy field, still fine
    print(user.name)         # COMPILE ERROR: use of moved field `name` from `user`
```

```

error: use of moved field `name` from `user`
```

**Why?** `user.name` is a `str`, which is a move type. Reading it transfers
ownership to `greeting`, so `user` no longer has a valid `name` field. The
`age` field is an `int32`, a copy type, so it is unaffected.

### Reading fields from borrowed values

You cannot move a non-copy field out of a borrowed value, because you do not
own it:

```aura fragment
def get_name(user: User) -> str:
    return user.name       # COMPILE ERROR
```

```

error: cannot move non-copy field `name` out of borrowed value `user`
```

The function only borrowed `user`, so it has no right to take `name` away.
Pick the fix that matches what you need.

**Option 1: clone the field.** The caller's `user` keeps its name:

```aura fragment
def get_name(user: User) -> str:
    return user.name.clone()   # explicit copy, user keeps its name
```

**Option 2: take ownership of the whole value.** The function consumes `user`:

```aura fragment
def get_name(user: own User) -> str:
    return user.name           # consumes user, moves name out
```

**Option 3: return a copy-type field instead.** Nothing moves:

```aura fragment
def get_age(user: User) -> int32:
    return user.age            # int32 is copy, no move needed
```

## Copy Classes

User-defined classes are move types by default. Write `copy class` to make a
class copyable. Every field must be a copy type:

```aura check-pass
copy class Point:
    x: int32
    y: int32

p1 = Point(x=1, y=2)
p2 = p1               # copies, both valid
print(p1.x)           # 1
print(p2.x)           # 1
```

If any field is a move type, the compiler rejects the class with `AU2002`:

```aura check-fail:AU2002
copy class Bad:
    name: str       # COMPILE ERROR
    value: int32
```

```

error: field `name` on `copy class Bad` must be a copy type, found `str`
```

**When to use `copy class`:** use it for small, value-like types where
copying is cheap and expected, such as coordinates, colors, dimensions, and
ranges. Do not use it for types that hold resources or large data.

## Borrowing In Loops

A bare `for` loop over a `list` or `set` borrows the collection, so the
collection stays usable after the loop:

```aura check-pass
mut names: list[str] = ["Ada", "Grace", "Margaret"]
for name in names:
    print(name)
print(names.len())     # 3 -- still usable
```

Write `own` when you intend to move each element out and consume the list:

```aura check-pass
def main():
    names: list[str] = ["Ada", "Grace", "Margaret"]
    for name in own names:
        print(name)
    # names is moved
```

A `list[int32]` is itself a move type, even though its elements are copy
types. Its bare loop still borrows. Only `own` consumes it:

```aura check-pass
mut xs: list[int32] = [1, 2, 3]
for x in xs:
    print(x)
for x in own xs:
    print(x)
# another use of xs would now be an error
```

### Bare shared iteration

Bare iteration is the shared form, so you can loop over the same collection
again:

```aura check-pass
mut names: list[str] = ["Ada", "Grace", "Margaret"]
for name in names:
    print(name)
print(names.len())     # 3 -- names is still valid

for name in names:   # can iterate again
    print(name)
```

For a copy element type, the loop variable receives a copy of each element.
For a non-copy element type, the loop variable is a temporary borrow.

### Mutable borrow iteration with `mut`

To modify elements during iteration, use `for ... in mut`. The collection
binding must be `mut`:

```aura check-pass
class Score:
    value: int32

    def double(mut self):
        self.value = self.value * 2

mut scores: list[Score] = [Score(value=1), Score(value=2), Score(value=3)]
for score in mut scores:
    score.double()

for score in scores:
    print(score.value)
# prints: 2, 4, 6
```

### Which iteration form to use

| Form | Effect | Use when |
|------|--------|----------|
| `for x in collection` | Shared borrow, collection stays valid | Ordinary read-only iteration |
| `for x in own collection` | Consumes the collection | You are done with the collection after the loop |
| `for x in mut collection` | Mutable borrow, can modify elements | You want to update elements in place |

### Comprehensions use the bare form

A comprehension is the eager expression form of nested bare loops:

```aura check-pass
names = ["Ada", "Grace"]
lengths = [name.len() for name in names]
copies = [name.clone() for name in names]
```

- The result collection is newly owned.
- A list or set clause shares its source and freezes it.
- `name.len()` only reads the shared `str`.
- Storing the non-copy `str` itself requires the explicit `.clone()` shown in
  `copies`. The compiler never inserts that clone.
- Comprehension clauses take no `mut` or `own` modifier. Use a statement loop
  to mutate or consume a collection.
- A Queue clause keeps the Queue bare-loop exception. Each received item
  arrives owned and may move directly into the eager result.
- Every loop target goes out of scope after the closing delimiter.

## Borrowing In Match

Pattern matching follows the same ownership rules. Bare `match` shares the
value, so the caller keeps ownership:

```aura check-pass
result: Result[str, str] = Result.Ok("success")
match result:
    case Ok(msg):
        print(msg)
    case Err(e):
        print(e)
print(result)          # still valid
```

Use `match own` to consume the value and receive owned payloads:

```aura check-pass
def main():
    result: Result[str, str] = Result.Ok("success")
    match own result:
        case Ok(msg):
            print(msg)     # msg is owned
        case Err(e):
            print(e)
    # result is moved
```

Use `match mut` to match and mutate the payload:

```aura check-pass
mut result: Result[str, str] = Result.Ok("hello")
match mut result:
    case Ok(msg):
        # msg is mut str -- can call mutating methods
        pass
    case Err(e):
        pass
```

## Closures Capture By Value

A lambda with a type from its context owns every outer local it uses:

```aura check-pass
def main():
    label = "compile"
    length: def() -> int64 = lambda: label.len()

    print(length())
    print(length())
```

- `label` moves into the closure when the lambda expression is evaluated.
- Both calls work because the body only reads its capture.
- If the body consumed a non-copy capture, a call would consume the closure.
  A second call would then report `AU3001`.

A copy capture is a snapshot and leaves its source usable. When outer code
also needs a non-copy value, clone it before creating the closure:

```aura check-pass
def main():
    label = "compile"
    captured = label.clone()
    length: def() -> int64 = lambda: captured.len()

    print(label)
    print(length())
```

Closures without a capture list follow these rules:

- Bare and `mut` parameters of the enclosing function are not captured. For a
  live shared or mutable loan, write an explicit, exhaustive capture list. See
  [Explicit Loan Captures](#explicit-loan-captures).
- A by-value closure may cross a task boundary only when every captured value
  is `Transfer`. `Transfer` is the compiler-derived rule for values that may
  cross between tasks. See [Borrowing And Concurrency](#borrowing-and-concurrency).
- A loan closure is always local and never `Transfer`.

A `def` type used for a stored value or an arbitrary parameter stays
capture-free. So a capturing closure has four valid homes:

- an immutable local
- a direct call
- a compiler-known repeatable callback
- one task start, for a qualifying closure moved into it

Do not route a capturing closure through a field, a collection, or an
annotated return. Those erase its environment metadata.

## Local Views, Reborrowing, And Inferred Lifetimes

A view names a live place without taking ownership. A place is a storage
location, such as a local, a field, a list element, or a dictionary entry:

```aura check-pass
class Counter:
    value: int64

def main():
    mut counter = Counter(value=1)
    view mut value = counter.value
    view mut nested = value
    nested = nested + 1
    print(counter.value)
```

`nested` is a reborrow of `value`, which means a view taken through another
view. Assigning to `nested` writes immediately to `counter.value`.

The compiler infers how long each loan lives. It ends both loans after their
final possible use. So the later read of `counter.value` is legal, even though
both view bindings are still in lexical scope.

Views follow these overlap rules:

- Shared views may overlap other shared views.
- A mutable view excludes all overlapping access to its source.
- Fields proven disjoint and fixed tuple positions can be loaned
  independently.

List elements and dictionary entries are view places too.
`view item = items[index]` and `view entry = table[key]` bind the selected slot
in place.

- Use `view mut` through a mutable source to update that slot.
- The selector is evaluated once, when the view is created. An invalid
  position or absent key traps with `AU4003`.
- Views of proven distinct literal positions or keys are disjoint.
- A computed selector overlaps every selector of the same collection.
- A structural mutation of the collection conflicts with every live element or
  entry view.

When the position or key may be missing, match on `lookup` instead of trapping.
The `Found` arm views the slot in place, and the `Missing` arm runs otherwise:

```aura check-pass
def main():
    mut teams = [["ada"], ["grace"]]
    match mut teams.lookup(5):
        case Lookup.Found(team):
            team.append("alan")
        case Lookup.Missing:
            teams.append(["alan"])
    print(teams.len())
```

`lookup` is only valid as the subject of a `match` statement. The view ends
with its arm, so the `Missing` arm may still change the list.

## Returned Views

A function can return access tied to one named receiver or parameter:

```aura check-pass
class User:
    name: str

def name(user: User) -> view str from user:
    return view user.name

def rename(user: mut User) -> view mut str from user:
    return view mut user.name

def main():
    mut user = User(name="Ada")
    view current = name(user)
    print(current)

    view mut editable = rename(user)
    editable = "Grace"
    print(user.name)
```

- The `from` origin is part of the function contract.
- A mutable result requires a mutable origin and a mutable view binding.
- A local, a temporary, an owned or defaulted parameter, or a different root
  cannot escape as the result.
- An ordinary `-> T` return stays owned.

## Explicit Loan Captures

A capture list is exhaustive. It names every captured value, which makes live
access visible:

```aura check-pass
class Counter:
    value: int64

    def add(mut self, amount: int64):
        self.value += amount

def main():
    mut counter = Counter(value=1)
    mut update: def(int64) -> None = lambda [mut counter] amount: counter.add(amount)
    update(2)
    update(3)
    print(counter.value)
```

| Capture | Meaning |
|---------|---------|
| `[counter]` | Shared loan |
| `[mut counter]` | Mutable loan |
| `[own counter]` | The original by-value copy or move capture |

A mutable-loan closure:

- is repeatable through a `mut` closure local, like `update` above
- keeps its source exclusively loaned until the closure's final use
- cannot enter a task, Queue, aggregate, or arbitrary structural `def`
  boundary

Run the combined example at
[examples/basics/views.au](../examples/basics/views.au).

## Borrowing And Concurrency

A queue takes ownership of each value you send. Putting a value into a queue
moves it:

```aura check-pass
jobs = Queue[str]()
jobs.put("hello")      # "hello" moves into the queue
# the sent string is now owned by whichever task receives it
```

Constructing a queue and sending on it require the payload type to satisfy the
compiler-derived `Transfer` rule:

- **Can cross:** copy values, `str`, and aggregates whose stored components
  are all `Transfer`.
- **Cannot cross:** `random.Rng`, `TaskGroup`, shared or mutable access, and
  live file, process, or network resources.

Keep a live resource on the task that owns it. Exchange owned descriptions,
bytes, snapshot results, or queue and task handles instead.

Queue handles are cheap copy references. Passing a queue to
`TaskGroup.start(...)` shares the same underlying queue, so the common case
needs no `.clone()`:

```aura check-pass
def send_message(jobs: Queue[str]):
    jobs.put("from task")
    jobs.close()

jobs = Queue[str]()
with TaskGroup() as group:
    task = group.start(send_message, jobs)
    match jobs.get():
        case QueueReceive.Item(value):
            print(value)   # "from task"
        case QueueReceive.Closed:
            pass
        case QueueReceive.TimedOut:
            pass
        case QueueReceive.Cancelled:
            pass
    task.result()
```

Every task argument and result must also be structurally `Transfer`. The
compiler checks this after generic specialization. A task target may borrow
from its task-owned capture through a bare parameter, but the captured value
itself crosses by ownership.

Observing a task result follows a separate repeatability rule:

- A copy result, a `Queue[...]` result, or a recursively repeatable
  `Task[...]` result may be observed repeatedly.
- For any other transferable result, `result()`, `poll()`, and `result_or()`
  consume the task handle on the first attempt. This holds even if that
  attempt times out, is cancelled, fails, or returns a fallback.
- For such results, `wait_any` and `wait_all` consume the complete task list.
  `wait_any` deliberately abandons the observation rights of the tasks it did
  not choose.

## Common Patterns And Fixes

### Pattern: "I need to use a value after passing it to a function"

This fails because `archive` takes ownership:

```aura fragment
def archive(doc: own Document):
    print(doc.title)

doc = Document(title="Report", pages=10)
archive(doc)
print(doc.title)       # COMPILE ERROR: use of moved value
```

**Fix 1:** remove `own` so the parameter uses the bare shared-borrow default:

```aura fragment
def archive(doc: Document):
    print(doc.title)
```

**Fix 2:** keep the owned parameter and pass a new value built from copies
of the fields. A class does not get a `.clone()` method automatically, so
clone the non-Copy fields yourself:

```aura fragment
archive(Document(title=doc.title.clone(), pages=doc.pages))
print(doc.title)       # doc still valid
```

### Pattern: "I need to read a str field without consuming the owner"

This fails because a shared borrow cannot give away its field:

```aura fragment
def get_title(doc: Document) -> str:
    return doc.title   # COMPILE ERROR: cannot move out of shared access
```

**Fix:** clone the field:

```aura fragment
def get_title(doc: Document) -> str:
    return doc.title.clone()
```

### Pattern: "I need to consume collection elements"

A bare loop only borrows, so the collection stays available:

```aura fragment
for item in items:
    inspect(item)
print(items.len())     # still available
```

**Fix:** use `own` when the consumer needs owned items:

```aura fragment
for item in own items:
    process(item)
# items is now moved
```

### Pattern: "I need to modify elements in a collection"

A bare loop gives shared access, so a mutating method fails:

```aura fragment
for score in scores:
    score.double()     # COMPILE ERROR: not mutable
```

**Fix:** iterate with a mutable borrow:

```aura fragment
for score in mut scores:
    score.double()
```

### Pattern: "The compiler says my binding must be mutable"

A mutating method needs a `mut` binding:

```aura fragment
counter = Counter(value=0)
counter.bump()         # COMPILE ERROR: must be a mutable place
```

**Fix:** declare the binding with `mut`:

```aura fragment
mut counter = Counter(value=0)
counter.bump()
```

## Mental Model For Python Developers

This table maps Python habits to Aura:

| Python concept | Aura equivalent |
|----------------|-------------------|
| `x = y` (always a reference) | `x = y` copies if copy type, moves if move type |
| `x = copy.deepcopy(y)` | `x = y.copy()` for collections; `x = y.clone()` for other clone-safe move types that expose it |
| `def f(x): ...` reads x | `def f(x: T): ...` for shared access |
| `def f(x): x.mutate()` | `def f(x: mut T): ...` |
| `del x` (deferred to GC) | Automatic when owner goes out of scope |
| `for x in list: ...` (list survives) | `for x in list: ...` (shared; list survives) |
| No direct equivalent | `for x in own list: ...` (list consumed) |

The key shift: in Python, assignment creates an alias. In Aura, assignment
transfers ownership. The rest of the system follows from that.

## Summary

1. Every value has one owner. When the owner goes out of scope, the value is
   freed.
2. Copy types, such as numbers, `bool`, and `Duration`, are duplicated on
   assignment. Move types, such as `str`, `list`, `random.Rng`, and classes,
   transfer ownership.
3. For an independent owned value, use collection `.copy()` or the `.clone()`
   method of another clone-safe move type. `random.Rng` and values containing
   it support neither.
4. Bare parameters grant shared access for every type. Use `mut T` to lend
   mutable access and `own T` to transfer ownership.
5. `mut` access is exclusive. No other overlapping access can exist at the
   same time.
6. Method receivers follow the same rules: `self` reads, `mut self` modifies,
   and `own self` consumes.
7. Bare collection iteration is shared. Use `for x in own collection` to
   consume and `for x in mut collection` to modify elements.
8. Use `match value` to pattern-match without consuming.
9. Queues transfer ownership of sent values and accept only structurally
   `Transfer` payloads. Queue handles are copy values.
10. Task captures and results must be structurally `Transfer`. A `Task[T]`
    handle is copyable only for a repeatable `T`. Otherwise the first result
    attempt consumes its unique observation right.

When the compiler reports a moved value or a borrowing error, come back to
this chapter. The fix is almost always one of the
[common patterns](#common-patterns-and-fixes).
