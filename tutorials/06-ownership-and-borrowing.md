# Ownership And Borrowing

If you are coming from Python, this is the most important chapter in the tutorial. Aurora does not use a garbage collector. Instead, it tracks who owns each value and when that value can be freed. This system is called **ownership**, and the way you temporarily lend values without giving them away is called **borrowing**.

This chapter walks through the full model with practical examples, explains why the rules exist, and shows you how to fix every common compiler error you will encounter.

## Why Ownership?

In Python, every value lives on a heap and a garbage collector cleans up when nothing points to it anymore. This is simple, but it has costs: unpredictable pauses, higher memory use, and no deterministic cleanup.

Aurora takes a different approach. Every value has exactly **one owner** at any point in time. When the owner goes out of scope, the value is freed immediately. No garbage collector, no reference counting, no surprises.

This gives you:

- **Predictable performance** -- no GC pauses
- **Deterministic cleanup** -- resources like files and connections close at a known point
- **Memory safety** -- the compiler rejects programs that would read freed or invalid memory

The trade-off is that you need to think about who owns what. The compiler enforces the rules and gives you clear error messages when something is wrong.

## Copy Types vs Move Types

Aurora divides all types into two categories: **copy types** and **move types**. Understanding this distinction is the foundation of everything that follows.

### Copy types

Copy types are small, fixed-size values that are cheap to duplicate. When you assign a copy type to a new binding or pass it to a function, Aurora silently makes a copy. Both the original and the new binding are fully independent.

The built-in copy types are:

- all integer types: `int` (the `int64` alias), `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`
- all unsigned types: `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`
- `float32`, `float64`
- `bool`
- `Duration`

Copy types behave the way Python developers expect:

```python
x: int32 = 10
y = x          # copies the value
print(x)       # 10 -- still usable
print(y)       # 10 -- independent copy
```

There is no surprise here. You can use `x` and `y` freely because integers are copy types.

### Move types

Move types are values that own heap-allocated data or manage a unique resource. When you assign a move type to a new binding, Aurora **moves** ownership. The original binding becomes invalid.

The built-in move types include:

- `String`
- `Vec[T]`, `Map[K, V]`, `Set[T]`
- `random.Rng`
- `TaskGroup`
- user-defined classes (by default)

`Queue[T]` and `Task[T]` are copy handles to shared runtime state. Copying a
handle does not copy a queued value or task result.

Here is where Python intuition breaks down:

```python
name: String = "aurora"
other = name          # ownership moves to `other`
print(other)          # "aurora" -- works fine
```

If you try to use `name` after the move:

```python
name: String = "aurora"
other = name
print(name)           # COMPILE ERROR
```

The compiler rejects this with:

```
error: use of moved value `name`
```

**Why does this happen?** After `other = name`, the `other` binding owns the string data. If `name` were still valid, you would have two bindings pointing to the same heap memory. When both go out of scope, the memory would be freed twice -- a crash. Aurora prevents this at compile time.

### The Python comparison

| Python | Aurora |
|--------|--------|
| `y = x` always creates a reference, both point to the same object | `y = x` copies for copy types, moves for move types |
| Garbage collector handles cleanup | Owner handles cleanup when it goes out of scope |
| You never think about who owns what | You always know who owns what |

## Cloning: Explicit Copies Of Move Types

When a move type supports independent duplication, call `.clone()`:

```python
name: String = "aurora"
other = name.clone()   # explicit copy -- name stays valid
print(name)            # "aurora"
print(other)           # "aurora"
```

Collections can be cloned too:

```python
mut xs: Vec[int32] = [1, 2, 3]
ys = xs.clone()        # independent copy
xs.push(4)
print(xs.len())        # 4
print(ys.len())        # 3 -- unaffected
```

`.clone()` is explicit because copying a large data structure is expensive. Aurora makes sure you know when you are paying that cost, unlike Python where every `=` on a list is a cheap reference but every mutation might surprise you via aliasing.

Move types are not automatically cloneable. `random.Rng` exposes no clone
route, and a class, enum, or collection containing one cannot be cloned through
a public clone-producing operation. Generic clone helpers infer this
requirement and reject an unsafe concrete specialization with `AU3007`.

## Passing Values To Functions

Bare function parameters are by value for copy types and shared borrows for
non-copy types. To transfer a move value to a function, write `own`:

```python
class Document:
    title: String
    pages: int32

def archive(doc: own Document):
    print(doc.title)

doc = Document(title="Report", pages=42)
archive(doc)
print(doc.pages)       # COMPILE ERROR: use of moved value `doc`
```

The explicit `own` parameter took ownership of `doc`. After the call, `doc` is no longer valid in the calling scope. If the declaration were simply `doc: Document`, it would borrow and the caller could keep using it.

For copy types, passing by value just copies:

```python
def double(x: int32) -> int32:
    return x * 2

value: int32 = 5
print(double(value))   # 10
print(value)           # 5 -- still valid, it was copied
```

## Borrowing: Lending Without Giving Away

Most of the time you want a function to read or modify a value without taking ownership. This is what **borrowing** does. A borrow is a temporary loan: the function can access the value, but the caller keeps ownership.

Aurora has two kinds of borrows:

- `borrow T` -- shared, read-only access
- `borrow mut T` -- exclusive, mutable access

### Explicit shared borrows with `borrow`

A shared borrow lets a function read a value without consuming it:

```python
class Counter:
    value: int32

def read(counter: borrow Counter) -> int32:
    return counter.value

mut counter = Counter(value=41)
print(read(counter))       # 41
print(counter.value)       # 41 -- counter still belongs to us
```

The `borrow` keyword makes the shared contract explicit: this function is just
looking, not taking. A bare non-copy parameter such as `counter: Counter` has
the same shared-borrow behavior; the explicit spelling is useful when you want
that intent to stand out. After the call returns, the borrow ends and the
caller still owns the value.

You can have multiple shared borrows active at the same time because none of them can modify the value:

```python
def sum_values(a: borrow Counter, b: borrow Counter) -> int32:
    return a.value + b.value

c1 = Counter(value=10)
c2 = Counter(value=20)
print(sum_values(c1, c2))   # 30 -- both still valid
```

### Mutable borrows with `borrow mut`

A mutable borrow lets a function modify the value in place:

```python
def bump(counter: borrow mut Counter):
    counter.value += 1

mut counter = Counter(value=41)
bump(counter)
print(counter.value)       # 42 -- the change persisted
```

The caller must declare the binding as `mut` because the function will modify it. If the binding is not mutable, the compiler rejects the call:

```python
counter = Counter(value=41)  # not mutable
bump(counter)                # COMPILE ERROR
```

```
error: argument for parameter `counter` in function `bump` must be a mutable place
```

### The exclusivity rule

You cannot have a `borrow mut` and any other borrow of the same value at the same time. This prevents data races and aliasing bugs:

```python
def bad(a: borrow mut Counter, b: borrow Counter):
    a.value += b.value

mut c = Counter(value=1)
bad(c, c)    # COMPILE ERROR: overlapping borrow
```

**Why does this rule exist?** Imagine `bad` increments `a.value` while reading `b.value` -- but `a` and `b` are the same object. The final result would depend on the order of operations inside the function, creating a subtle bug. Aurora prevents this entirely.

Think of it like a library book: many people can read it at the same time (shared borrows), or one person can take it home to annotate it (mutable borrow), but you cannot do both at once.

## Method Receivers

Methods on classes use the same borrowing system through **receivers**. The receiver determines what the method can do with the instance:

### `self` -- read the instance

```python
class Account:
    balance: float64

    def display(self) -> String:
        return f"Balance: {self.balance}"
```

Bare `self` is a shared borrow. The method can read fields but cannot modify
them, and the caller retains ownership. `borrow self` is accepted as an
explicit synonym when spelling out the shared contract helps readability.

```python
account = Account(balance=100.0)
print(account.display())    # "Balance: 100.0"
print(account.balance)      # still accessible
```

### `borrow mut self` -- modify the instance

```python
class Account:
    balance: float64

    def deposit(borrow mut self, amount: float64):
        self.balance += amount

    def display(self) -> String:
        return f"Balance: {self.balance}"
```

The method can read and write fields. The instance must be declared `mut`:

```python
mut account = Account(balance=100.0)
account.deposit(50.0)
print(account.display())    # "Balance: 150.0"
```

If you forget `mut`:

```python
account = Account(balance=100.0)
account.deposit(50.0)       # COMPILE ERROR: must be a mutable place
```

### `own self` -- consume the instance

```python
class Connection:
    host: String

    def into_host(own self) -> String:
        return self.host
```

An `own self` receiver takes ownership. A non-copy instance is consumed after the call:

```python
conn = Connection(host="example.com")
host = conn.into_host()
print(host)               # "example.com"
print(conn.host)          # COMPILE ERROR: use of moved value `conn`
```

Use `own self` when the method needs to disassemble the instance or transfer ownership of its fields.

### No receiver -- associated methods

Methods without a receiver are called on the class itself, not on an instance:

```python
class Counter:
    value: int32

    def zero() -> Counter:
        return Counter(value=0)
```

```python
c = Counter.zero()
```

### Choosing the right receiver

| Receiver | When to use | Example |
|----------|-------------|---------|
| `self` | Read-only shared access, the default | getters, display, serialization |
| `borrow self` | Explicit synonym for shared `self` | emphasizing a shared contract |
| `borrow mut self` | Modify the instance in place | setters, increment, append |
| `own self` | Consume the instance to extract data | `into_*` conversions, one-shot use |
| no receiver | Factory methods, utilities that don't need an instance | `Counter.zero()` |

If you are not sure, start with bare `self`. Add `own` only when the method
must consume the instance, or `borrow mut` when it must mutate in place.

## Field Access And Move Semantics

When you own a value, reading a non-copy field **moves** that field out of the instance:

```python
class User:
    name: String
    age: int32

user = User(name="Ada", age=36)
greeting = user.name         # moves `name` out of `user`
print(greeting)              # "Ada"
print(user.age)              # 36 -- copy field, still fine
print(user.name)             # COMPILE ERROR: use of moved field `name` from `user`
```

```
error: use of moved field `name` from `user`
```

**Why?** The `String` in `user.name` is a move type. Reading it transfers ownership to `greeting`. The `user` instance no longer has a valid `name` field. The `age` field is `int32` (a copy type), so it is unaffected.

### Reading fields from borrowed values

When you borrow a value, you cannot move non-copy fields out of it because you do not own it:

```python
def get_name(user: borrow User) -> String:
    return user.name       # COMPILE ERROR
```

```
error: cannot move non-copy field `name` out of borrowed value `user`
```

The function only borrowed `user` -- it has no right to take the `name` away. The fix depends on what you need:

**Option 1: clone the field**

```python
def get_name(user: borrow User) -> String:
    return user.name.clone()   # explicit copy, user keeps its name
```

**Option 2: take ownership of the whole value**

```python
def get_name(user: own User) -> String:
    return user.name           # consumes user, moves name out
```

**Option 3: return a copy-type field instead**

```python
def get_age(user: borrow User) -> int32:
    return user.age            # int32 is copy, no move needed
```

## Copy Classes

By default, user-defined classes are move types. You can make a class copyable with `copy class`, but only if every field is itself a copy type:

```python
copy class Point:
    x: int32
    y: int32

p1 = Point(x=1, y=2)
p2 = p1               # copies, both valid
print(p1.x)           # 1
print(p2.x)           # 1
```

If any field is a move type, the compiler rejects the `copy` annotation:

```python
copy class Bad:
    name: String       # COMPILE ERROR
    value: int32
```

```
error: field `name` on `copy class Bad` must be a copy type, found `String`
```

**When to use `copy class`:** Use it for small, value-like types where copying is cheap and expected -- coordinates, colors, dimensions, ranges. Do not use it for types that hold resources or large data.

## Borrowing In Loops

Loops use the same readable default. Bare `Vec` and `Set` iteration borrows the
collection, so it remains usable:

```python
mut names: Vec[String] = ["Ada", "Grace", "Margaret"]
for name in names:
    print(name)
print(names.len())     # 3 -- still usable
```

Write `own` when you intend to move each element out and consume the vector:

```python
names: Vec[String] = ["Ada", "Grace", "Margaret"]
for name in own names:
    print(name)
# names is moved
```

**Note:** Even `Vec[int32]` is itself a move type, but its bare loop still
borrows. Only `own` consumes it:

```python
mut xs: Vec[int32] = [1, 2, 3]
for x in xs:
    print(x)
for x in own xs:
    print(x)
# another use of xs would now be an error
```

### Explicit shared iteration with `borrow`

Bare iteration is already shared; `for ... in borrow` makes that contract explicit:

```python
mut names: Vec[String] = ["Ada", "Grace", "Margaret"]
for name in borrow names:
    print(name)
print(names.len())     # 3 -- names is still valid

for name in borrow names:   # can iterate again
    print(name)
```

The `borrow` keyword tells Aurora to iterate over borrowed references. The collection stays owned by the caller.

For copy element types, the loop variable receives a copy of each element. For non-copy element types, the loop variable is a temporary borrow.

### Mutable borrow iteration with `borrow mut`

To modify elements during iteration, use `for ... in borrow mut`:

```python
class Score:
    value: int32

    def double(borrow mut self):
        self.value = self.value * 2

mut scores: Vec[Score] = [Score(value=1), Score(value=2), Score(value=3)]
for score in borrow mut scores:
    score.double()

for score in borrow scores:
    print(score.value)
# prints: 2, 4, 6
```

This requires the collection binding to be `mut`.

### Which iteration form to use

| Form | Effect | Use when |
|------|--------|----------|
| `for x in collection` | Shared borrow, collection stays valid | Ordinary read-only iteration |
| `for x in own collection` | Consumes the collection | You are done with the collection after the loop |
| `for x in borrow collection` | Explicit shared borrow | You want the borrow visible in source |
| `for x in borrow mut collection` | Mutable borrow, can modify elements | You want to update elements in place |

**Default recommendation:** Use bare `for x in collection` for reads, `own` to consume, and `borrow mut` to update.

## Borrowing In Match

Pattern matching follows the same ownership rules. By default, `match` takes ownership of the value:

```python
result: Result[String, String] = Result.Ok("success")
match result:
    case Ok(msg):
        print(msg)
    case Err(e):
        print(e)
print(result)          # COMPILE ERROR if result is non-copy: already moved
```

To match without consuming the value, use `match borrow`:

```python
result: Result[String, String] = Result.Ok("success")
match borrow result:
    case Ok(msg):
        print(msg)     # msg is a borrowed reference
    case Err(e):
        print(e)
# result is still valid here
```

To match and mutate the payload, use `match borrow mut`:

```python
mut result: Result[String, String] = Result.Ok("hello")
match borrow mut result:
    case Ok(msg):
        # msg is borrow mut String -- can call mutating methods
        pass
    case Err(e):
        pass
```

## Borrowing And Concurrency

Queues transfer ownership of sent values. When you put a value into a queue, it moves:

```python
jobs = Queue[String]()
jobs.put("hello")      # "hello" moves into the queue
# the sent string is now owned by whichever task receives it
```

Queue and task handles are cheap copy-like references. Passing a queue to `TaskGroup.start(...)` shares the same underlying queue; you do not need `.clone()` for the common case:

```python
def send_message(jobs: Queue[String]):
    jobs.put("from task")
    jobs.close()

jobs = Queue[String]()
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

Queue and task handles are cheap copy-like values, so the maintained surface does not require `.clone()` when passing them around.

## Common Patterns And Fixes

### Pattern: "I need to use a value after passing it to a function"

**Problem:**
```python
def archive(doc: own Document):
    print(doc.title)

doc = Document(title="Report", pages=10)
archive(doc)
print(doc.title)       # COMPILE ERROR: use of moved value
```

**Fix 1 -- remove `own` to use the bare shared-borrow default:**
```python
def archive(doc: Document):
    print(doc.title)
```

Writing `doc: borrow Document` is an equivalent explicit shared spelling.

**Fix 2 -- keep the owned parameter and clone before passing:**
```python
archive(doc.clone())
print(doc.title)       # doc still valid
```

### Pattern: "I need to read a String field without consuming the owner"

**Problem:**
```python
def get_title(doc: borrow Document) -> String:
    return doc.title   # COMPILE ERROR: cannot move from borrow
```

**Fix -- clone the field:**
```python
def get_title(doc: borrow Document) -> String:
    return doc.title.clone()
```

### Pattern: "I need to consume collection elements"

**Problem:**
```python
for item in items:
    inspect(item)
print(items.len())     # still available
```

**Use `own` when the consumer needs owned items:**
```python
for item in own items:
    process(item)
# items is now moved
```

### Pattern: "I need to modify elements in a collection"

**Problem:**
```python
for score in borrow scores:
    score.double()     # COMPILE ERROR: not mutable
```

**Fix -- mutable borrow iterate:**
```python
for score in borrow mut scores:
    score.double()
```

### Pattern: "The compiler says my binding must be mutable"

**Problem:**
```python
counter = Counter(value=0)
counter.bump()         # COMPILE ERROR: must be a mutable place
```

**Fix -- declare with `mut`:**
```python
mut counter = Counter(value=0)
counter.bump()
```

## Mental Model For Python Developers

Here is how to translate your Python intuition:

| Python concept | Aurora equivalent |
|----------------|-------------------|
| `x = y` (always a reference) | `x = y` copies if copy type, moves if move type |
| `x = copy.deepcopy(y)` | `x = y.clone()` when `y` supports clone and is clone-safe |
| `def f(x): ...` reads x | `def f(x: T): ...` for non-copy `T`, or explicitly `def f(x: borrow T): ...` |
| `def f(x): x.mutate()` | `def f(x: borrow mut T): ...` |
| `del x` (deferred to GC) | Automatic when owner goes out of scope |
| `for x in list: ...` (list survives) | `for x in list: ...` (shared; list survives) |
| No direct equivalent | `for x in own list: ...` (list consumed) |

The key shift is: in Python, assignment creates aliases. In Aurora, assignment transfers ownership. Once you internalize this, the rest of the system follows naturally.

## Summary

1. Every value has one owner. When the owner goes out of scope, the value is freed.
2. Copy types (numbers, `bool`, `Duration`) are duplicated on assignment. Move types (`String`, `Vec`, `random.Rng`, classes) transfer ownership.
3. Use `.clone()` when you need an explicit independent copy and the move type
   supports clone; `random.Rng` and values containing it do not.
4. Bare non-copy parameters are shared borrows. Use `borrow T` to make that
   read-only contract explicit, and `borrow mut T` to lend mutable access.
5. `borrow mut` is exclusive -- no other borrows of the same value can exist at the same time.
6. Method receivers follow the same rules: `self` (or `borrow self`) reads, `borrow mut self` modifies, and `own self` consumes.
7. Bare collection iteration is shared. Use `for x in own collection` to consume and `for x in borrow mut collection` to modify elements.
8. Use `match borrow value` to pattern-match without consuming.
9. Queues transfer ownership of sent values. Queue and task handles are cheap copy-like values, so sharing the handle itself does not require an explicit clone.

The compiler enforces all of these rules. When you see an error about moved values or borrowing, come back to this chapter -- the fix is almost always one of the patterns listed above.
