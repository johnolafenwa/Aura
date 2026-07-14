# Aurora

**A readable, Python-inspired systems language with Rust-like memory safety and Go-like concurrency**

> **Status: historical design proposal.** This document records the original target design and is not the implemented language specification. The canonical 0.1 contract is the maintained [Status and Compatibility](manual/status-and-compatibility.md) page plus the Manual and [Current Limits](manual/current-limits.md). When this proposal differs from those documents or the compiler fixtures, the maintained 0.1 sources win.

## Executive summary

Aurora is a programming language designed for developers who love Python’s readability but want the performance, safety, and deployment model of a modern compiled systems language.

Aurora is **not** a Python runtime, **not** a CPython replacement, and **not** a compatibility layer for the Python ecosystem. It has:

- Python-inspired, indentation-based syntax
- static-only semantics
- native compilation
- ownership and borrowing for memory safety
- lightweight runtime-managed tasks inspired by goroutines
- structured concurrency through `TaskGroup`, `Task`, and `Queue[T]`
- a first-party package and workspace manager; registry publishing remains future work
- a standard library that feels familiar to Python developers where semantics genuinely align

Aurora’s goal is to occupy a very specific design space:

> **The clarity of Python, the safety model of Rust, and the concurrency ergonomics of Go, without inheriting CPython’s runtime constraints or ecosystem baggage.**

This document is written as a concrete implementation proposal intended for an implementation agent or engineering team.

---

# 1. Language vision

## 1.1 Core philosophy

Aurora should feel natural to Python programmers on first read, but it must be honest about being a different language.

This means:

- Code should look clean and readable
- Semantics should be static, explicit, and optimizable
- Memory safety should be enforced by the compiler
- Concurrency should be built into the language and runtime
- Build, package, test, format, and documentation workflows should be first-party
- Familiarity should come from syntax and module naming, not from pretending to be Python internally

## 1.2 Non-goals

Aurora explicitly does **not** aim to:

- run existing Python code unchanged
- support CPython extensions
- be compatible with Python object semantics
- import or install existing Python packages
- preserve Python’s dynamic typing model
- preserve Python’s reference semantics for objects
- reproduce the entire Python standard library
- provide a full interpreter in v1

## 1.3 Primary use cases

Aurora should be especially strong for:

- backend services
- network servers
- command line tools
- data-processing pipelines
- infrastructure systems
- ML tooling and systems components
- concurrent applications
- high-performance developer tools

---

# 2. Name rationale

## 2.1 Chosen name: Aurora

**Aurora** is a strong fit because it suggests:

- a new beginning rather than a derivative clone
- clarity, light, and readability
- speed and modernity
- a broad and elegant identity suitable for a language, compiler, registry, and package toolchain

The ecosystem can use names like:

- `aurora` for the compiler driver
- `aura` for the package manager and workspace tool
- `registry.aurora-lang.org` for the package registry
- `.au` as the source extension

Alternative names that also fit the concept:

- Luma
- Valea
- Pyra
- Halo
- Vanta

But **Aurora** is the recommended choice because it feels new rather than derivative.

---

# 3. Design principles

Aurora should obey the following design principles.

## 3.1 Readability over cleverness

The syntax should reward clear code. Indentation-based blocks, meaningful keywords, and minimal punctuation should make code easy to scan.

## 3.2 Static semantics only

Aurora should not have a dynamic execution mode. All variables, expressions, and functions participate in a static type system.

Type inference is welcome, but runtime dynamism is not.

## 3.3 Zero-cost abstractions where practical

High-level constructs should compile down efficiently. Generics, iterators, task spawning, channels, and pattern matching should not force unnecessary runtime overhead.

## 3.4 Safety by default

The default path should be the safe path. Memory safety, data-race safety, and structured concurrency should be the norm.

## 3.5 Honest syntax

The language should look Pythonic, but when semantics differ from Python in important ways, the syntax must reveal that difference.

Examples:

- mutation should be explicit
- moves should be explicit or inferable under strict rules
- concurrency should use dedicated primitives like `spawn`, `Channel[T]`, and `select`

## 3.6 One official toolchain

The language should ship with a first-party workflow for:

- creating projects
- dependency management
- package publishing
- testing
- formatting
- linting
- documentation generation
- benchmarking

---

# 4. High-level language shape

## 4.1 Surface syntax

Aurora uses:

- indentation-based blocks
- `def` for functions
- `class`, `enum`, and `trait` declarations
- `impl Trait for Type` blocks for trait conformance
- direct binding for immutable values
- `mut` for mutable bindings
- `import` and module paths inspired by Python
- explicit type annotations where useful
- type inference where unambiguous

### Example

```python
import math
import net.http

class Point:
    x: float64
    y: float64

def distance(a: Point, b: Point) -> float64:
    dx = a.x - b.x
    dy = a.y - b.y
    return math.sqrt(dx * dx + dy * dy)
```

This should look familiar to a Python developer while remaining fully static and compiled.

## 4.2 Classes

Aurora uses `class` for nominal product types.

In v1, a class has these semantics:

- it is a value type by default, not a Python-style reference object
- it does not imply heap allocation or object identity
- assignment of a non-copy class value moves ownership
- methods are declared inside the class body
- inheritance is not part of v1
- `impl Trait for Type` exists only for trait conformance, not for inherent methods

## 4.3 File and project layout

Source file extension: `.au`

Suggested package layout:

```text
myapp/
  Aurora.toml
  src/
    main.au
    lib.au
    net/
      server.au
  tests/
  benches/
```

---

# 5. Semantic foundations

This section is the most important. The implementation should prioritize these rules before surface polish.

## 5.1 Values, bindings, and ownership

Aurora uses ownership-based semantics inspired by Rust.

A binding owns a value unless it is borrowing it.

### Rules

1. Every value has one logical owner unless shared through explicit safe mechanisms.
2. Assignment transfers ownership for non-copy types unless the type is trivially copyable or explicitly cloned.
3. Borrowing must be explicit in the type system and checked by the compiler.
4. Mutable access must be exclusive.
5. Shared immutable access may be aliased.

### Example

```python
mut xs = Vec[int32]([1, 2, 3])
ys = xs         # move
# xs is no longer usable after this line
```

Cloning remains explicit:

```python
mut ys = Vec[int32]([1, 2, 3])
zs = ys.clone()
# ys is still usable because clone creates a second owned value
```

## 5.2 Mutability

Mutability must be opt-in.

### Rules

- `x = expr` introduces a new immutable binding only when `x` is not already bound in the current scope
- `mut x = expr` introduces a new mutable binding
- `x = expr` against an existing binding is reassignment, not a new declaration
- reassignment is legal only for bindings originally declared with `mut`
- shadowing local bindings is not part of v1; reusing a name in the same function body should be treated as reassignment or rejected with a clear diagnostic
- bindings are immutable by default
- fields are immutable unless declared mutable or accessed through a mutable owner
- interior mutability should exist only through explicit library types

### Example

```python
x = 10
# x = 20      # error: `x` is immutable

mut y = 20
y = 30

# mut y = 40  # error: this is not a new declaration; `y` already exists
```

## 5.3 Copy vs move vs borrow

Types fall into one of these broad categories:

- **copy types**: small plain-value types such as integers, booleans, some small tuples
- **move types**: heap-owning or resource-owning types like vectors, strings, sockets, files
- **borrowed references**: temporary non-owning access

Canonical borrow syntax:

```python
borrow T       # shared borrow
borrow mut T   # exclusive mutable borrow
```

Aurora uses the `borrow` keyword in both type positions and expression positions:

```python
name: borrow str = ...
reader = borrow config
writer = borrow mut config
```

Borrowing must stay visually obvious.

Applying `borrow` to an already borrowed value creates a reborrow rather than a nested surface type such as `borrow borrow T`.

Reborrows still obey the ordinary exclusivity rules:

- `borrow` of a shared or mutable borrow creates a temporary shared reborrow
- `borrow mut` requires mutable access and may not be derived from a shared borrow

Shared borrows allow read-only aliasing:

```python
def show_total(xs: borrow Vec[int32]) -> int32:
    return xs.len()
```

Exclusive mutable borrows allow mutation, but they must be unique while active:

```python
mut counter = Counter(value=0)

reader = borrow counter
# writer = borrow mut counter   # error: cannot take a mutable borrow while `reader` is alive
```

```python
mut counter = Counter(value=0)

writer = borrow mut counter
writer.value += 1
# reader = borrow counter       # error: cannot take a shared borrow while `writer` is alive
# other = borrow mut counter    # error: cannot take a second mutable borrow while `writer` is alive
```

Once the mutable borrow ends, borrowing again is valid:

```python
def bump(counter: borrow mut Counter):
    counter.value += 1

mut counter = Counter(value=0)
bump(borrow mut counter)
snapshot = borrow counter
```

## 5.4 Lifetimes

Initial implementation should avoid exposing explicit lifetime syntax unless absolutely necessary.

Preferred approach:

- infer lifetimes where possible
- expose explicit lifetime parameters only in advanced APIs
- keep most user code lifetime-annotation free

This is a major ergonomics differentiator.

Canonical elision rules for v1:

- if a function has exactly one borrowed input and returns a borrowed value, the returned borrow is tied to that input
- if a method returns a borrowed value and has a borrowed receiver, the returned borrow is tied to `self` unless another source is stated explicitly
- if multiple borrowed inputs could be the source of the returned borrow and elision would be ambiguous, the API must use explicit lifetime parameters in advanced code rather than relying on inference

## 5.5 Destruction and resource cleanup

Aurora should use deterministic destruction for owned values.

When a value goes out of scope, its destructor runs automatically.

This enables safe management of:

- memory
- files
- sockets
- locks
- channels
- OS handles

Aurora uses `with` as the scoped cleanup mechanism in v1.

Leaving a `with` block runs the cleanup path deterministically on normal completion and on propagated errors.

`with` is Aurora's general scoped enter/exit construct. It is used both for ordinary resource cleanup and for structured task scopes such as `with task_group() as group:`.

There is no general-purpose `defer` construct in v1.

Conceptually, `with` lowers through a standard-library protocol like:

```python
trait With[T]:
    def enter(borrow mut self) -> T
    def exit(borrow mut self)
```

`with name = expr:` evaluates `expr`, calls `enter()`, binds the entered value to `name`, and always calls `exit()` on scope exit. The runtime may use specialized implementations for types like task groups, but the user-facing model stays trait-like rather than ad hoc.

Example:

```python
def read_file(path: borrow str) -> Result[String, IoError]:
    with file = try fs.open(path):
        return file.read_all()
```

## 5.6 Class value semantics

Classes are nominal product types with ownership-aware value semantics.

Rules:

- class values are move types by default
- a class may be declared with `copy class Name:` only when all of its fields are themselves copy types; copy is explicit in v1 and is not inferred automatically
- passing a class by value into a function or returning it by value moves ownership unless the class is `copy`
- method calls do not create hidden aliasing; shared `self`/`borrow self`, consuming `own self`, and mutable `borrow mut self` obey normal ownership and borrow rules
- field access through a borrowed receiver yields borrowed access for non-copy fields and copied values for copy fields; moving a non-copy field out of `self`, `borrow self`, or `borrow mut self` is illegal unless an explicit extraction operation is defined
- class fields are stored inline by default
- direct recursive class fields are illegal; recursive structures use the built-in `indirect` storage modifier
- `indirect T` means the field owns a `T` value stored indirectly rather than inline; moving the outer object moves ownership of that indirect child
- plain classes do not have implicit object identity; if shared identity is desired, it must be modeled explicitly through types such as `Arc[T]`, `Mutex[T]`, or other library abstractions

In practice, there are only a few things you can do with fields through a borrow:

- copy a field if that field's type is `copy`
- return or pass along a borrowed view of a non-copy field
- clone a non-copy field explicitly if you need a second owned value
- mutate a field in place through `borrow mut self`

What you cannot do is silently move a non-copy field out of a borrowed object.

Example `copy` class:

```python
copy class Color:
    r: uint8
    g: uint8
    b: uint8
```

Example:

```python
class Node:
    value: int32
    next: indirect Node?       # optional indirect child

class User:
    id: uint64
    name: String

    def user_id(self) -> uint64:
        return self.id         # valid: `uint64` is a copy type

    def name_view(self) -> borrow str:
        return self.name.as_str()

    def name_copy(self) -> String:
        return self.name.clone()

    def rename(borrow mut self, new_name: String):
        self.name = new_name   # valid: mutate in place through an exclusive mutable borrow

def into_name(user: User) -> String:
    return user.name           # valid: `user` is owned by this function

# invalid:
# def bad_name(user: borrow User) -> String:
#     return user.name         # illegal: cannot move a non-copy field out of a borrow

class Counter:
    value: int32

    def read(self) -> int32:
        return self.value
```

How to read this example:

- `Node.next` uses `indirect Node?` because a class cannot contain itself directly. Without `indirect`, the type would have infinite size.
- `indirect T` means the field owns a `T`, but stores it out of line instead of inline inside the parent object.
- `Node?` means the field is optional, so a node may or may not have a next node.
- `user_id` is valid because `id` is a `uint64`, and `uint64` is a copy type. Bare `self` is a shared receiver, so reading it copies the value.
- `name_view` is valid because it does not take ownership of the `String`; it returns a borrowed string view instead.
- `name_copy` is valid because `.clone()` creates a new owned `String` while leaving the original field in place.
- `rename` is valid because `borrow mut self` gives exclusive mutable access, so replacing a field in place is allowed.
- `into_name` is valid because the function owns `user`. Moving `user.name` out is allowed when the whole object is owned.
- `bad_name` is invalid because `borrow User` only gives temporary access. Moving `user.name` out would partially empty a value that the function does not own.

This is the core distinction:

- from `User`, you may move non-copy fields out
- from `borrow User`, you may inspect, borrow, or clone non-copy fields, but not move them out
- from `borrow mut User`, you may inspect, borrow, clone, or mutate in place, but not move fields out without an explicit language mechanism that leaves the object valid

For recursive data structures, Aurora prefers `indirect` over exposing a wrapper type in ordinary code:

```python
class Node:
    value: int32
    next: indirect Node?
```

This should be read as "an optional owned child node stored indirectly."

---

# 6. Type system

## 6.1 Static-only typing

Aurora is statically typed. There is no `Any`-driven dynamic fallback in the core language.

Type inference is allowed, but every expression has a compile-time-known type.

## 6.2 Type inference

Inference should make local code pleasant, but public APIs should encourage explicitness.

Recommended rule:

- local bindings may omit type annotations when inferable
- function parameters should be explicit
- function return types should be explicit unless the function returns `None`
- exported public items should require explicit signatures

## 6.3 Generics

Aurora should support parametric polymorphism for containers, algorithms, channels, futures if any, and user-defined abstractions.

Example:

```python
class Cell[T]:
    value: T

def first[T](xs: borrow [T]) -> borrow T:
    return xs[0]
```

Generics should compile efficiently through monomorphization where appropriate.

Monomorphization means the compiler generates specialized concrete code for each generic type use, such as one version of a function for `Vec[int32]` and another for `Vec[String]`.

## 6.4 Traits

Aurora needs a way to express shared behavior without inheritance-heavy design.

Aurora uses `trait` as the single behavior-abstraction keyword in v1.

Needs to support:

- generic constraints
- operator overloading through traits
- formatting/printing
- comparison
- hashing
- iteration
- async/concurrency safety traits if needed

Example:

```python
trait Display:
    def format(borrow self, w: borrow mut Writer)
```

Trait implementations use explicit conformance blocks:

```python
impl Display for User:
    def format(borrow self, w: borrow mut Writer):
        w.write(self.name.as_str())
```

Canonical v1 syntax for generic constraints uses inline bounds:

```python
def sort[T: Ord](xs: borrow mut [T]):
    ...
```

Aurora may add trailing `where` clauses later, but inline `T: Trait` bounds are the frozen v1 form.

Multiple bounds use `+`:

```python
def render_sorted[T: Display + Ord](xs: borrow [T]):
    ...
```

Operator overloading is expressed through ordinary traits:

```python
trait Add[Rhs, Out]:
    def add(self, rhs: Rhs) -> Out

impl Add[Point, Point] for Point:
    def add(self, rhs: Point) -> Point:
        return Point(x=self.x + rhs.x, y=self.y + rhs.y)
```

## 6.5 Enums and algebraic data types

Aurora should have first-class sum types.

Example:

```python
enum Result[T, E]:
    Ok(T)
    Err(E)

enum Option[T]:
    Some(T)
    None
```

This is essential for error handling, protocols, and compiler-friendly exhaustive matching.

Aurora also uses `Option[T]` for optional values, and `T?` is shorthand for `Option[T]` in type positions.

## 6.6 Pattern matching

Aurora should include pattern matching from v1.

Canonical v1 rules:

- `match value:` matches by value and may move non-copy payloads out of the scrutinee
- `match borrow value:` and `match borrow mut value:` borrow the scrutinee instead of consuming it
- bindings introduced by a by-value match receive owned values for move types and copied values for copy types
- bindings introduced by a borrowed match are borrowed values

Example:

```python
match borrow result:
    case Result.Ok(value):
        print(value)
    case Result.Err(err):
        log.error(err)
```

Pattern matching is important because it gives a clean static way to work with enums and structured data.

## 6.7 Type aliases and newtypes

The language should support:

- type aliases for readability
- newtypes / wrapper classes for stronger domain modeling

## 6.8 Strings

Aurora uses two canonical UTF-8 string forms in v1:

- `String` for owned text
- `borrow str` for borrowed string slices

Rules:

- string literals have type `borrow str`
- APIs that inspect text should usually accept `borrow str`
- APIs that construct or store text should return or contain `String`
- converting borrowed text to owned text is explicit, for example `String("hello")`

## 6.9 Slices

Borrowed slices are a future design target for copy-avoiding access to contiguous data; they are not implemented in Aurora 0.1.

- `borrow [T]` is a borrowed slice of elements of type `T`
- slices are non-owning views and do not allocate
- slices are the preferred parameter type for read-only access to vectors and buffers

Examples:

```python
def sum(xs: borrow [int32]) -> int32:
    total = 0
    for x in xs:
        total += x
    return total
```

```python
def starts_with(data: borrow [uint8], prefix: borrow [uint8]) -> bool:
    ...
```

## 6.10 None and unit

Aurora uses `None` as the unit type and as the sole value of that type.

- `None` means "no meaningful value"
- `None` is a copy type
- `Result[None, E]` is the standard way to express success-or-error when success carries no payload

Aurora also uses `None` as the empty `Option[T]` variant. The meaning is resolved by type context:

```python
x: Option[int32] = None   # the empty option variant
y: None = None          # the unit value
```

This is not a runtime ambiguity. `None` has no payload in either role, so the compiler resolves it statically from the expected type.

## 6.11 String interpolation

Aurora supports string interpolation with `f"..."`.

Rules:

- an f-string produces an owned `String`
- interpolated expressions are formatted through the `Display` trait or equivalent formatting trait
- interpolation borrows values for formatting where possible; it does not implicitly move non-copy values merely to print or format them

Example:

```python
def greet(name: borrow str) -> String:
    return f"Hello, {name}"
```

## 6.12 Tuples

Aurora supports anonymous tuple types for small fixed-size aggregates.

Rules:

- tuple syntax uses `(T1, T2, ...)` for types and `(v1, v2, ...)` for values
- tuple elements are accessed with zero-based dotted indices such as `pair.0` and `pair.1`
- tuples are copy when all of their element types are copy
- tuples otherwise follow normal move semantics
- tuple literals may be destructured in pattern matching and bindings in later phases

Example:

```python
pair: (int32, bool) = (1, true)
named = (String("Aurora"), 1)
first = pair.0
```

## 6.13 Primitive numeric types

Aurora provides a default signed integer spelling plus explicit primitive numeric widths in v1:

- default signed integer: `int`, an alias for `int64`
- signed integers: `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`
- unsigned integers: `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`
- floating-point: `float32`, `float64`
- boolean: `bool`

`int` is an alias for `int64`, and an unsuffixed integer literal defaults to `int64` when no expected integer type is available. An expected type still wins, so explicitly declared `int32` APIs and literals passed to them remain `int32`.

## 6.14 Fixed-size arrays

Fixed-size owned arrays are deferred until after v1.

Aurora v1 standard sequence types are:

- `Vec[T]` for owned growable sequences
- `borrow [T]` for borrowed contiguous slices

---

# 7. Functions and methods

## 7.1 Functions

Functions use `def`.

Rules:

- `def name(...):` is shorthand for `def name(...) -> None:`
- reaching the end of a `None`-returning function is equivalent to `return`
- `return value` requires a non-`None` return type
- `return` with no value is only valid in `None`-returning functions
- executable entry files may use top-level executable statements instead of an explicit `main`
- a file may not mix top-level executable statements with an explicit `main` in v1

Example:

```python
def add(x: int32, y: int32) -> int32:
    return x + y

def log_total(total: int32):
    print(total)
```

Script-style entry files are also allowed:

```python
a: int32 = 6
b: int32 = 10
print(a + b)
```

## 7.2 Methods

Inherent methods are declared directly inside the `class` body.

Example:

```python
class Counter:
    value: int32 = 0

    def inc(borrow mut self):
        self.value += 1
```

The self model must make ownership and mutability clear.

Recommended receiver kinds:

- `self` for shared borrow by default
- `borrow self` as an explicit synonym for shared borrow
- `own self` for by-value consumption
- `borrow mut self` for exclusive mutable borrow

The typed spelling `self: SomeType` is not a receiver and is rejected with a
diagnostic naming these forms.

Associated methods omit a receiver and are called through the class name.

## 7.3 Closures

Closures should be supported.

They must obey ownership rules for captured values.

Capture modes could be:

- inferred when safe
- explicitly movable for spawned tasks

Example:

```python
x = 10
f = |y| x + y
```

Spawning closures should usually move captured values unless borrowed under strict scoped rules.

## 7.4 Iteration and for loops

Aurora uses a trait-based iteration protocol.

Aurora distinguishes between iterable values and iterator objects.

Core rules:

- `for x in expr:` consumes `expr` by value unless `expr` is explicitly borrowed
- `for x in borrow expr:` iterates by shared borrow
- `for x in borrow mut expr:` iterates by mutable borrow
- iterable values implement an `Iterable[T, IterT: Iterator[T]]`-style capability and provide consuming `into_iter(own self)` to yield an iterator object
- iterator objects provide `next(borrow mut self) -> Option[T]`
- if `expr` already has a borrowed type, `for x in expr:` uses that borrowed iteration behavior
- `for x in borrow expr:` where `expr` is already borrowed is treated as a reborrow, not as a nested `borrow borrow ...` type
- shared borrowed iteration yields copied element values for copy element types and `borrow T` for non-copy element types
- mutable borrowed iteration yields `borrow mut T` elements
- borrowed iteration works through ordinary `Iterable` implementations for borrowed receiver types such as `borrow [T]` and `borrow Vec[T]`, not through a separate compiler-only escape hatch

This lets ownership stay explicit:

- iterating over `Vec[T]` by value may consume the vector
- iterating over `borrow [int32]` yields copied `int32` values because `int32` is copy
- iterating over `borrow [String]` yields `borrow String` elements
- iterating over a channel receives values until the channel is closed

Example:

```python
trait Iterator[T]:
    def next(borrow mut self) -> Option[T]

trait Iterable[T, IterT: Iterator[T]]:
    def into_iter(own self) -> IterT

for value in range(4):
    print(value)

for item in borrow xs:
    print(item)
```

## 7.5 Constructors

Aurora uses value constructors rather than Python-style `__init__`.

Rules:

- every class may use keyword field construction, such as `Point(x=1.0, y=2.0)`, when the relevant fields are visible
- constructor calls produce fully initialized values directly
- classes may define associated constructor methods such as `new`, `default`, `empty`, or `from_file`
- fallible constructors return `Result[Self, E]`
- default argument expressions are evaluated at the call site in left-to-right parameter order
- default values may not reference other parameters in v1
- trait method declarations do not use default arguments in v1
- partial initialization is not part of v1

Example:

```python
class User:
    public name: String
    public age: int32 = 0

    def new(name: String, age: int32 = 0) -> Self:
        return Self(name=name, age=age)

    def guest() -> Self:
        return Self(name=String("Guest"), age=0)
```

## 7.6 Control flow

Aurora uses Python-style conditionals and loop syntax.

Rules:

- conditionals use `if`, `elif`, and `else`
- looping supports both `for` and `while`
- loops support `break` and `continue`
- conditions must have type `bool`; Aurora does not use truthy/falsy coercion in v1

Example:

```python
if score > 90:
    print("A")
elif score > 80:
    print("B")
else:
    print("C")

while remaining > 0:
    if remaining == 3:
        break
    if remaining % 2 == 0:
        remaining -= 1
        continue
    remaining -= 1
```

---

# 8. Concurrency model

Aurora should adopt Go-like concurrency as a first-class language and runtime feature.

## 8.1 Goals

The concurrency model should be:

- easy to use
- safe by default
- efficient
- compatible with ownership semantics
- suitable for I/O concurrency now and explicitly staged parallel computation later

## 8.2 Tasks instead of asyncio-style coroutines

Aurora should not use Python `async`/`await` as the primary concurrency model.

Instead, it should use lightweight runtime-managed tasks.

Canonical primitive:

```python
spawn worker(job)
```

or

```python
task = spawn worker(job)
```

Tasks should be much lighter than OS threads and scheduled by the Aurora runtime.

## 8.3 Channels

Channels are a core runtime abstraction exposed as `Channel[T]`.

`Channel[T]` is a lightweight handle to shared channel state.

Rules:

- channel handles are move types
- `channel.clone()` creates an additional handle to the same underlying channel
- channels support multiple producers and multiple consumers in v1
- `.send()`, `.recv()`, and `.close()` borrow the handle rather than consuming it
- `.send(value)` returns `Result[None, SendError[T]]`; a send to a closed channel fails and the error carries the unsent value
- `.recv()` blocks until a value is available or the channel is closed and drained, then returns `T?`
- closing any handle closes the underlying channel for the whole channel instance
- channel iteration repeatedly calls `.recv()` and stops after it returns `None`

Sharing a channel across tasks uses cloned handles:

```python
with task_group() as group:
    group.spawn(worker, jobs.clone(), results.clone())
    group.spawn(worker, jobs.clone(), results.clone())
```

Example:

```python
jobs = Channel[int32](capacity=100)
results = Channel[String](capacity=100)
```

Operations:

- send
- receive
- close
- iteration over channel
- bounded and unbounded variants if desired

Canonical API style:

```python
send_result = jobs.send(42)
next_value = results.recv()
```

Aurora uses method-call channel operations in v1. There is no alternate operator syntax for send or receive.

## 8.4 Select

Aurora should have a `select` construct for waiting on multiple channel operations.

Example:

```python
while running:
    select:
        case msg = inbox.recv():
            match msg:
                case Some(value):
                    handle(value)
                case None:
                    break
        case sig = shutdown.recv():
            match sig:
                case Some(_):
                    break
                case None:
                    break
        case after(1s):
            heartbeat()
```

This is essential for service programming and structured concurrent systems.

Aurora also supports duration literals for time-related APIs:

- `500ms`
- `1s`
- `2m`

These construct `Duration` values and may be used in places such as timers, deadlines, and `after(...)`.

## 8.5 Structured concurrency

Aurora uses structured concurrency in v1.

Core rules:

- a parent scope owns child tasks unless explicitly detached
- task groups enable spawning related work and waiting for completion
- leaving a task group waits for child completion or cancels remaining work on failure
- cancellation propagates through task hierarchies
- detached background work must be explicit with `spawn detached`

Example sketch:

```python
with task_group() as group:
    group.spawn(fetch_user, id)
    group.spawn(fetch_orders, id)
```

This prevents runaway background tasks from becoming a language-wide footgun.

## 8.6 Task safety and ownership

Spawning work must interact cleanly with ownership.

Rules:

- values captured by spawned tasks are moved by default
- borrowed references may only be captured if the compiler can prove the task does not outlive the borrow
- detached tasks may not capture borrowed references in v1
- mutable shared state requires explicit safe shared types

## 8.7 Shared state

Aurora should support shared state, but not as the default mental model.

Standard library primitives:

- `Mutex[T]`
- `RwLock[T]`
- `Arc[T]` or equivalent atomic shared ownership type
- `Atomic*` primitives

These should require explicit use.

## 8.8 Threads vs tasks

Aurora runtime tasks are the default concurrency abstraction.

OS threads should exist as an advanced implementation detail or lower-level API, but most users should not need them directly.

## 8.9 Parallelism

Because Aurora is compiled and not constrained by a Python GIL, tasks should be schedulable across multiple CPU cores.

The runtime should support true parallel execution.

---

# 9. Error handling

Aurora should avoid exception-heavy design as the default path.

## 9.1 Result-based errors

Primary recoverable errors should use a `Result[T, E]` style.

Example:

```python
def parse_int(s: borrow str) -> Result[int32, ParseError]:
    ...
```

## 9.2 Error propagation

Aurora uses `try expr` for recoverable error propagation.

`try expr` means:

- if `expr` is `Ok(value)`, continue with `value`
- if `expr` is `Err(err)`, return `Err(err)` from the current function

Example:

```python
def load_config(path: borrow str) -> Result[Config, IoError]:
    text = try fs.read_to_string(path)
    return parse_config(text)
```

## 9.3 Error conversion

If `try expr` produces `Err(source_err)` and the current function returns `Result[T, TargetError]`, Aurora first checks for an exact error-type match.

If the types differ, Aurora applies a `From[SourceError] for TargetError` conversion when one exists.

Example:

```python
trait From[T]:
    def from(value: T) -> Self

def load_and_parse(path: borrow str) -> Result[Config, AppError]:
    text = try fs.read_to_string(path)   # IoError converts into AppError
    config = try parse_config(text)      # ParseError converts into AppError
    return Ok(config)
```

If no exact match or `From` conversion exists, `try expr` is a type error.

## 9.4 Panics or fatal errors

Aurora may include unrecoverable failures for internal bugs or violated invariants, but these should be clearly separate from ordinary error handling.

## 9.5 Exceptions

Recommendation: avoid general-purpose exceptions in v1.

This keeps control flow explicit and compiler-friendly.

---

# 10. Modules, packages, and imports

## 10.1 Module system

Aurora should have a clean hierarchical module system with Python-friendly import syntax.

Example:

```python
import json
import math
import net.http
from collections import HashMap
```

The internal implementation need not follow Python rules exactly, but the import surface should feel familiar.

## 10.2 Packages

Each package is a compile unit with explicit metadata.

Canonical manifest:

```toml
[package]
name = "myapp"
version = "0.1.0"
edition = "2026"

[dependencies]
http = "1.2"
jsonx = "0.4"
```


Filename: `Aurora.toml`

## 10.3 Workspaces

Support multi-package workspaces from early on.

This matters for larger codebases, monorepos, and compiler development itself.

## 10.4 Visibility

Aurora uses `public` for exported APIs.

Rules:

- top-level items are module-private by default
- class fields and methods are private by default
- `public` may be applied to classes, enums, traits, functions, fields, and methods
- the synthesized keyword field constructor is public only when the class is public and the participating fields are public
- public classes should prefer named constructors when invariants matter

---

# 11. Standard library strategy

## 11.1 Philosophy

The standard library should be familiar to Python programmers **where semantics align**, but it should not pretend to replicate Python where doing so would be misleading.

## 11.2 Reuse friendly names where appropriate

Good candidates for familiar naming:

- `math`
- `json`
- `pathlib`
- `datetime`
- `collections`
- `string`
- `bytes`
- `fs`
- `net`
- `time`
- `random`
- `fmt`
- `hashing`

Python-compatible names can be used when behavior is close enough to user expectations.

## 11.3 Do not copy Python modules whose semantics do not fit

Avoid or replace direct analogues for:

- `asyncio`
- `threading`
- `multiprocessing`
- `gc`
- `inspect`
- dynamic `types`-style reflection modules

Aurora should define libraries that match Aurora semantics, not inherited Python semantics.

## 11.4 Essential v1 standard library modules

Recommended v1 modules:

- `math`
- `cmp`
- `collections`
- `string`
- `bytes`
- `pathlib`
- `fs`
- `io`
- `time`
- `datetime`
- `json`
- `toml`
- `os`
- `process`
- `net`
- `net.http`
- `sync`
- `task`
- `fmt`
- `test`

A small default prelude may re-export a few extremely common names, such as `print`, while their canonical library home remains explicit modules like `fmt`.

`print(...)` is an ordinary library function, not a macro. In v1 it is the standard line-printing helper and appends a trailing newline. It formats its arguments through the formatting traits and should borrow values for formatting where possible.

## 11.5 Reflection

Keep reflection deliberately limited in v1.

Full runtime introspection often fights static compilation, optimization, and safety.

---

# 12. Memory model

## 12.1 Rust-like ownership and borrowing

Aurora should use an ownership-based memory model rather than garbage collection.

## 12.2 Heap allocation

Heap allocation should be explicit through standard containers and owned types but not painfully verbose.

## 12.3 Shared ownership

Allow shared ownership only via explicit atomic or reference-counted types.

Example:

```python
shared = Arc[Config](config)
```

## 12.4 Unsafe code

Aurora should include an `unsafe` escape hatch eventually, but keep it minimal and isolated.

Use cases:

- FFI
- custom allocators
- low-level runtime internals
- specialized performance work

The safe subset should be the default and should cover most application code.

---

# 13. Interoperability

## 13.1 C ABI first

Aurora should target excellent C ABI interoperability in v1.

FFI stands for Foreign Function Interface. It is the mechanism that lets Aurora code call functions written in another language, and lets code in another language call Aurora functions across a defined binary boundary.

That means:

- calling C functions
- exporting Aurora functions with stable C ABI
- representing strings, slices, and plain data classes in FFI-friendly ways
- safe wrappers over unsafe interop boundaries

## 13.2 C++ later

Direct C++ interoperability should not be a v1 requirement.

C++ adds major complexity through name mangling, templates, exceptions, object layout, and ABI instability.

A C ABI bridge is the correct initial target.

## 13.3 Python interoperability not required

No special accommodation is required for CPython, Python packages, or the Python C API.

---

# 14. Runtime

Aurora does not need an interpreter, but it does need a runtime.

## 14.1 Runtime responsibilities

The runtime should handle:

- task scheduling
- channel coordination
- timers
- network event integration
- task parking and waking
- possibly cooperative plus preemptive scheduling points
- panic handling
- startup and shutdown hooks

## 14.2 Runtime constraints

The runtime should be:

- small
- predictable
- efficient
- portable
- independent from a dynamic object system

## 14.3 No full interpreter mode in v1

Instead of a true interpreter, Aurora should aim for:

- fast incremental builds
- fast startup
- excellent error messages
- REPL later if desired using JIT or eval subsets

## 14.4 Task-aware I/O

Aurora's standard network and timer APIs should be task-aware.

Recommended v1 behavior:

- when a task waits on supported network I/O, the runtime parks that task rather than blocking an OS thread
- the runtime uses platform event mechanisms such as epoll, kqueue, or IOCP behind ordinary library APIs
- Aurora does not require a separate `async`/`await` syntax for its primary I/O model
- explicitly blocking FFI or OS calls may still block the underlying thread unless wrapped in runtime-aware libraries

---

# 15. Compilation model

## 15.1 Native compilation

Aurora compiles to native machine code.

Suggested backend options:

- LLVM for initial implementation speed and portability
- Cranelift later for faster debug builds if desired

## 15.2 Build modes

Recommended modes:

- debug
- release
- test
- benchmark

## 15.3 Incremental compilation

This should be a major priority. One of Python’s biggest ergonomic strengths is fast iteration. Aurora should recover some of that through tooling.

## 15.4 Separate compilation and caching

Packages and modules should compile incrementally with aggressive caching.

---

# 16. Toolchain

## 16.1 First-party tools

Recommended tools:

- `aurora` compiler driver
- `aura` package manager / workspace tool
- `aurfmt` formatter
- `aurlint` linter
- `aurdoc` documentation tool
- `aurtest` test runner if not folded into `aura test`

To reduce fragmentation, prefer subcommands under one main tool:

```bash
aura new myapp
aura build
aura run
aura test
aura fmt
aura lint
aura doc
aura publish
```

## 16.2 Registry

Aurora should have a first-party registry similar in spirit to Cargo.

Requirements:

- package publishing
- semantic versioning
- lockfiles
- dependency resolution
- checksums
- private registries eventually

## 16.3 Lockfile

Use a lockfile for reproducible builds.

Lockfile filename:

- `Aura.lock`

---

# 17. Syntax sketch

This section provides a rough syntax target. The implementation team may refine specifics, but the semantics should remain.

## 17.1 Bindings

```python
x = 10
mut y = 20
name: String = String("Aurora")
```

## 17.2 Classes

```python
class User:
    id: uint64
    name: String
    email: String
```

## 17.3 Enums

```python
enum Message:
    Ping
    Text(String)
    Data(Bytes)
```

## 17.4 Functions

```python
def greet(name: borrow str) -> String:
    return f"Hello, {name}"
```

## 17.5 Methods

```python
class User:
    id: uint64
    name: String
    email: String

    def display(borrow self) -> borrow str:
        return self.name.as_str()
```

## 17.6 Match

```python
match borrow msg:
    case Message.Ping:
    print("ping")
    case Message.Text(text):
        print(text)
    case Message.Data(data):
        print(data.len())
```

## 17.7 Spawning tasks

```python
def worker(id: int32, jobs: Channel[Job], out: Channel[Result[String, Error]]):
    for job in jobs:
        out.send(process(job))    # ignoring send failure for brevity

def main() -> Result[None, Error]:
    jobs = Channel[Job](capacity=100)
    out = Channel[Result[String, Error]](capacity=100)

    with task_group() as group:
        for i in range(4):
            group.spawn(worker, i, jobs.clone(), out.clone())

        for job in load_jobs():
            jobs.send(job)        # ignoring send failure for brevity

        jobs.close()

    return Ok(None)
```

## 17.8 Select

```python
while running:
    select:
        case msg = inbox.recv():
            match msg:
                case Some(value):
                    handle(value)
                case None:
                    break
        case err = errors.recv():
            match err:
                case Some(value):
                    log.error(value)
                case None:
                    break
        case after(500ms):
            print("tick")
```

## 17.9 Conditionals and while

```python
if ready:
    run()
elif retry:
    wait()
else:
    fail()

while remaining > 0:
    if remaining == 3:
        break
    if remaining % 2 == 0:
        remaining -= 1
        continue
    remaining -= 1
```

## 17.10 Tuples

```python
point = (3.0, 4.0)
pair: (int32, bool) = (1, true)
x = point.0
```

---

# 18. Error messages and developer experience

Aurora should invest heavily in diagnostics.

Goals:

- readable compiler errors
- specific ownership diagnostics
- suggestions for move vs borrow vs clone fixes
- module resolution hints
- concurrency misuse diagnostics
- beginner-friendly explanations

This matters because ownership systems live or die by error quality.

---

# 19. Testing and documentation

## 19.1 Tests

Built-in test support should exist from v1.

Aurora uses a small built-in attribute syntax spelled `@name` on declarations.

`@test` is a built-in v1 attribute for the test runner. General user-defined decorators or arbitrary compile-time attributes are not part of v1.

Example:

```python
@test
def test_add():
    assert add(2, 3) == 5
```

## 19.2 Documentation comments

Provide doc comments and first-party documentation generation.

Example:

```python
## Returns the Euclidean distance between two points.
def distance(a: Point, b: Point) -> float64:
    ...
```

---

# 20. Implementation roadmap for Codex

This section is the concrete build plan.

## Phase 0: Decision freeze

Before writing the full compiler, freeze these decisions. The rest of this document assumes the following v1 answers:

1. Source files use the `.au` extension.
2. Package metadata lives in `Aurora.toml`.
3. Bindings use `x = 10` and `mut y = 20`; there is no `let` keyword; first assignment in a scope introduces the binding, later assignment is reassignment, and local shadowing is not part of v1.
4. Borrow syntax is `borrow T` and `borrow mut T`.
5. Assignment of non-copy values moves ownership.
6. Nominal product types use `class`; inherent methods live in class bodies; `impl Trait for Type` is reserved for trait conformance; classes move by default, `copy class Name:` is the explicit copy spelling, and recursive fields use the built-in `indirect` storage modifier.
7. Pattern matching is by value unless the scrutinee is explicitly borrowed with `match borrow value:` or `match borrow mut value:`.
8. `for` loops use a trait-based iteration model; `for x in expr:` consumes by value unless `expr` is explicitly borrowed.
9. Concurrency uses `spawn`, `task_group`, `Channel[T]`, and `select`; detached tasks require explicit `spawn detached`; channel sharing uses cloned handles over shared channel state; `.send()` returns `Result[None, SendError[T]]`; `.recv()` returns `T?`; and ordinary network/timer APIs are task-aware rather than split into a separate `async` dialect.
10. Recoverable errors use `Result[T, E]`, `try expr`, and `None` as the unit success payload when no value is needed.
11. Text uses `String` for owned values, `borrow str` for borrowed string slices, `borrow [T]` for borrowed contiguous slices, and `f"..."` to produce owned interpolated strings.
12. Visibility uses `public`, with items private by default.
13. The initial standard library uses `string`, `bytes`, `collections`, `task`, `sync`, `fs`, `fmt`, `json`, and related core modules.
14. Scoped cleanup uses `with`; the same construct is also used for managed task scopes; there is no general-purpose `defer` in v1.
15. Generic constraints use inline `T: Trait` bounds with `+` for multiple bounds; `try expr` supports `From[SourceError] for TargetError` conversions; and default arguments are evaluated at the call site and are not part of trait method declarations in v1.
16. Functions may omit `-> None`, and executable entry files may use top-level statements instead of an explicit `main`, but a file may not use both entry styles at once in v1.
17. Control flow uses Python-style `if`/`elif`/`else` plus `while`; loops support `break` and `continue`; conditions must be `bool`; and tuple syntax uses `(T1, T2, ...)` with dotted numeric field access.
18. Primitive numeric types use names such as `int`, `int32`, `uint64`, `uintsize`, and `float64`; `int` aliases `int64`, unsuffixed integer literals default to `int64`, and fixed-size owned arrays are deferred until after v1.
19. `with` lowers through a standard enter/exit protocol, and `@test` is part of a small built-in attribute syntax rather than a general decorator system in v1.

Do not start implementation until these decisions are treated as fixed.

## Phase 1: Minimal viable language front end

Implement:

- lexer
- indentation-sensitive parser
- AST
- module loader
- name resolution
- basic type checker

Support only:

- integers
- booleans
- `String` and `borrow str`
- functions
- local variables
- immutable and mutable bindings
- classes
- simple methods
- `public` visibility
- imports
- conditionals
- loops

## Phase 2: Ownership and borrowing core

Implement:

- owned values
- move semantics
- copy semantics for primitive types
- shared borrows
- mutable borrows
- borrow checker for local scopes
- deterministic destruction

At this phase, correctness is more important than ergonomics.

## Phase 3: Mid-level IR and code generation

Implement:

- typed IR
- lowering from AST to IR
- monomorphization for generics or placeholder design if generics come later
- LLVM backend for native code generation
- debug and release modes

## Phase 4: Core standard library

Implement:

- `String` and string utilities
- vectors
- hash maps
- options/results
- file I/O
- path handling
- time
- formatting
- JSON and TOML parsing if feasible

## Phase 5: Error handling and pattern matching

Implement:

- `Result`
- `try expr`
- enums
- exhaustive `match`

## Phase 6: Concurrency runtime

Implement:

- task scheduler
- channels
- select
- timers
- task spawning
- runtime shutdown behavior

Initially keep the scheduler simple but correct.

## Phase 7: Package manager and registry tooling

Implement:

- package manifest parsing
- dependency resolution
- lockfile
- build graph
- project creation templates
- local package cache

Registry publishing can come after local package workflows.

## Phase 8: Diagnostics and polish

Implement:

- high-quality compiler errors
- formatter
- documentation generator
- test runner
- benchmarks

## Phase 9: FFI and unsafe subset

Implement:

- C ABI import/export
- safe wrappers over raw pointers
- minimal `unsafe` blocks

---

# 21. Reference implementation architecture

Recommended architecture:

## 21.1 Compiler layers

1. lexer
2. parser
3. AST
4. name resolution
5. type checking
6. ownership/borrow checking
7. MIR or similar mid-level IR
8. optimization passes
9. backend code generation
10. linker integration

## 21.2 Runtime layers

1. task scheduler
2. channel subsystem
3. timers and clock
4. I/O integration layer
5. panic/error runtime support
6. memory and allocator integration

## 21.3 Tooling layers

1. manifest parser
2. dependency resolver
3. build graph
4. cache manager
5. formatter parser integration
6. documentation extractor

---

# 22. Recommended v1 constraints

To keep the project implementable, v1 should explicitly exclude:

- macros beyond very small built-ins
- full reflection
- inheritance-based OOP
- exceptions
- garbage collection
- CPython interop
- direct C++ interop
- JIT compilation
- interpreted mode
- metaprogramming heavy features
- advanced lifetime syntax unless absolutely necessary

This is important. A good v1 is narrow and coherent.

---

# 23. Frozen language decisions

These decisions are frozen for v1 and should not be reopened casually during implementation.

## 23.1 Binding syntax

Aurora uses direct bindings:

- `x = 10`
- `mut y = 20`

There is no `let` keyword in v1.

Additional frozen rules:

- first assignment to a name in a local scope introduces that binding
- later `x = expr` reuses the existing binding and is reassignment, not a new declaration
- reassignment requires the binding to have been declared with `mut`
- local shadowing is not part of v1

## 23.2 Borrow syntax

Aurora uses `borrow T` and `borrow mut T`.

This keeps borrowing visually explicit in an indentation-based language and reads more naturally than sigils.

Method receivers use bare `self` for the common shared case, `borrow self` as
its explicit synonym, `own self` for consumption, and `borrow mut self` for
exclusive mutation.

## 23.3 Product type keyword

Aurora uses `class` for nominal product types.

A `class` is a value type by default. It does not imply inheritance, hidden heap allocation, or Python-style object identity.

Additional frozen rules:

- classes move by default when passed, assigned, or returned by value
- `copy class Name:` is the explicit copy spelling in v1
- `copy` is only legal when all fields are copy types
- direct recursive class fields are illegal; recursion uses the built-in `indirect` storage modifier
- `T?` is shorthand for `Option[T]` in type positions, so recursive fields commonly look like `indirect Node?`
- accessing a non-copy field through shared `self` or `borrow self` does not move that field out of the object
- shared identity must be modeled through explicit wrapper types rather than plain classes

## 23.4 Trait and iteration model

Aurora uses `trait` for behavior declarations.

Inherent methods are written inside class bodies. Trait implementations use explicit `impl Trait for Type` blocks.

Additional frozen rules:

- `for` loops use a trait-based iteration protocol
- iterable values provide consuming `into_iter(own self)` and iterator objects provide `next(borrow mut self) -> Option[T]`
- `for x in expr:` iterates by value unless `expr` is explicitly borrowed
- multiple trait bounds use `T: Trait1 + Trait2`
- `for x in borrow expr:` over an already borrowed iterable is a reborrow
- shared borrowed iteration yields copied element values for copy element types and `borrow T` for non-copy element types
- mutable borrowed iteration yields `borrow mut T` elements
- borrowed iteration uses ordinary `Iterable` implementations for borrowed receiver types rather than compiler-only special cases

## 23.5 String, slice, and interpolation model

Aurora uses:

- `String` for owned UTF-8 text
- `borrow str` for borrowed UTF-8 string slices
- `borrow [T]` for borrowed contiguous slices

String literals have type `borrow str`. Converting borrowed text to owned text is explicit.

Additional frozen rules:

- `f"..."` produces an owned `String`
- interpolation formats values through formatting traits and borrows where possible rather than moving just to print or format

## 23.6 Visibility

Aurora uses `public` for exported APIs.

Items are private by default.

## 23.7 Constructor model

Aurora constructors create fully initialized values directly.

The language supports:

- keyword field construction
- associated constructor methods that return `Self`
- fallible constructors that return `Result[Self, E]`
- default arguments evaluated at the call site
- no default arguments in trait method declarations in v1

Aurora does not use Python-style `__init__`.

## 23.8 Match ownership model

`match value:` matches by value and may move non-copy payloads.

`match borrow value:` and `match borrow mut value:` borrow the scrutinee instead.

## 23.9 Historical task and channel model

The original design below used `Channel`, `spawn`, and `select`. It has been superseded in the implemented 0.1 surface by `Queue[T]`, `TaskGroup.start(...)`, `TaskGroup.start_soon(...)`, `wait_any(...)`, and `wait_all(...)`. Aurora 0.1 has no detached-task form.

Aurora uses structured concurrency in v1.

- `spawn` creates a child task in the current task scope
- `task_group` owns related child tasks
- detached background work must be explicit
- `with` is the general scoped enter/exit construct and is also used for task groups
- channels use the `Channel[T]` type
- channel handles are move types that refer to shared channel state
- channel sharing uses `.clone()` to create additional handles to the same channel
- channels are multi-producer and multi-consumer in v1
- channel operations use methods such as `.send()`, `.recv()`, and `.close()`, and those operations borrow the handle
- `.send()` returns `Result[None, SendError[T]]` and preserves the unsent value on failure
- `.recv()` returns `T?`, blocking until a value is available or the channel is closed and drained
- `select` supports duration literals such as `500ms`, `1s`, and `2m` through `after(...)`
- ordinary network and timer APIs are task-aware; Aurora does not use a separate `async`/`await` model for primary I/O

## 23.10 Result and unit model

`Result[T, E]` is a standard-library enum with language support for `try expr`.

Aurora uses `None` as the unit type and as the sole value of that type.

`Option[T]` uses `Some(T)` and `None`, and the meaning of `None` is resolved from type context.

If `try expr` returns an error whose type does not exactly match the caller's error type, Aurora uses a `From[SourceError] for TargetError` conversion when available.

## 23.11 Scoped cleanup

Aurora uses `with` for scoped cleanup in v1.

The same `with` construct is also used for managed task scopes such as `with task_group() as group:`.

`with` lowers through a standard enter/exit protocol rather than a one-off special form for each type.

There is no general-purpose `defer`.

## 23.12 Control flow and proposed tuples

Aurora uses Python-style `if`/`elif`/`else` and supports `while` loops in v1.

Conditions must have type `bool`.

Loops support `break` and `continue`.

Tuple syntax was proposed here but is not implemented in Aurora 0.1.

## 23.13 Primitive types and proposed attributes

Aurora v1 primitive scalar type spellings are `bool`, `int`, `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`, `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`, `float32`, and `float64`.

`int` is an alias for `int64`; unsuffixed integer literals default to `int64` when they have no expected integer type. Explicit fixed-width API contracts, including those that use `int32`, are unchanged.

Fixed-size owned arrays are deferred until after v1.

Attribute syntax and `@test` were proposed here but are not implemented in Aurora 0.1.

---

# 24. Canonical language pitch

Use this as the short public description:

> **Aurora is a readable, Python-inspired compiled language with explicit ownership and structured concurrency. It is designed for developers who want clean syntax, typed failure, native deployment, and one modern toolchain. Aurora 0.1 task execution is cooperative and single-threaded.**

---

# 25. Final guidance to Codex

Build Aurora as a coherent language, not as a Python emulator.

When faced with a choice between:

- preserving superficial Python familiarity
- and maintaining clean static semantics

choose clean static semantics.

When faced with a choice between:

- magical ergonomics
- and explicit safe behavior

choose explicit safe behavior.

When faced with a choice between:

- supporting many paradigms weakly
- and supporting one strong mental model well

choose one strong mental model.

The winning mental model is:

- readable Pythonic syntax
- static types
- ownership and borrowing
- task-based concurrency
- channel-based coordination
- native compilation
- first-party tooling

That is Aurora.

---

# 26. Suggested first milestone

The first milestone should be a tiny but complete subset capable of this:

```python
class Point:
    x: float64
    y: float64

def distance(a: Point, b: Point) -> float64:
    dx = a.x - b.x
    dy = a.y - b.y
    return (dx * dx + dy * dy).sqrt()

def main() -> int32:
    p1 = Point(x=0.0, y=0.0)
    p2 = Point(x=3.0, y=4.0)
    print(distance(p1, p2))
    return 0
```

An equivalent script-style milestone entry file could omit `main` entirely:

```python
p1 = Point(x=0.0, y=0.0)
p2 = Point(x=3.0, y=4.0)
print(distance(p1, p2))
```

Once that works with parsing, type checking, compilation, and execution, move on to ownership, collections, `Result`, and tasks.

---

# 27. Closing note

Aurora should not try to be everything.

It should be one thing done well:

**a beautiful, safe, concurrent compiled language that feels familiar to Python programmers without being bound by Python’s past.**
