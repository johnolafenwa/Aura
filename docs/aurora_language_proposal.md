# Aurora

**A readable, Python-inspired systems language with Rust-like memory safety and Go-like concurrency**

## Executive summary

Aurora is a new programming language designed for developers who love Python’s readability but want the performance, safety, and deployment model of a modern compiled systems language.

Aurora is **not** a new Python runtime, **not** a CPython replacement, and **not** a compatibility layer for the Python ecosystem. It is a new language with:

- Python-inspired, indentation-based syntax
- static-only semantics
- native compilation
- ownership and borrowing for memory safety
- lightweight runtime-managed tasks inspired by goroutines
- channel-based concurrency and `select`
- a first-party package manager and registry
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
    x: f64
    y: f64

def distance(a: Point, b: Point) -> f64:
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
mut xs = Vec[int]([1, 2, 3])
ys = xs         # move
# xs is no longer usable after this line
```

Cloning remains explicit:

```python
mut ys = Vec[int]([1, 2, 3])
zs = ys.clone()
# ys is still usable because clone creates a second owned value
```

## 5.2 Mutability

Mutability must be opt-in.

### Rules

- bindings are immutable by default
- fields are immutable unless declared mutable or accessed through a mutable owner
- interior mutability should exist only through explicit library types

### Example

```python
x = 10
mut y = 20
y = 30
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

Shared borrows allow read-only aliasing:

```python
def show_total(xs: borrow Vec[int]) -> i32:
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

There is no general-purpose `defer` construct in v1.

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
- a class may be marked `copy` only when all of its fields are themselves copy types; copy is explicit in v1 and is not inferred automatically
- passing a class by value into a function or returning it by value moves ownership unless the class is `copy`
- method calls do not create hidden aliasing; `self`, `borrow self`, and `borrow mut self` obey normal ownership and borrow rules
- field access through a borrowed receiver yields borrowed access for non-copy fields and copied values for copy fields; moving a non-copy field out of `borrow self` or `borrow mut self` is illegal unless an explicit extraction operation is defined
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

Example:

```python
class Node:
    value: i32
    next: indirect Node?       # optional indirect child

class User:
    id: u64
    name: String

    def user_id(borrow self) -> u64:
        return self.id         # valid: `u64` is a copy type

    def name_view(borrow self) -> borrow str:
        return self.name.as_str()

    def name_copy(borrow self) -> String:
        return self.name.clone()

    def rename(borrow mut self, new_name: String):
        self.name = new_name   # valid: mutate in place through an exclusive mutable borrow

def into_name(user: User) -> String:
    return user.name           # valid: `user` is owned by this function

# invalid:
# def bad_name(user: borrow User) -> String:
#     return user.name         # illegal: cannot move a non-copy field out of a borrow

class Counter:
    value: i32

    def read(borrow self) -> i32:
        return self.value
```

How to read this example:

- `Node.next` uses `indirect Node?` because a class cannot contain itself directly. Without `indirect`, the type would have infinite size.
- `indirect T` means the field owns a `T`, but stores it out of line instead of inline inside the parent object.
- `Node?` means the field is optional, so a node may or may not have a next node.
- `user_id` is valid because `id` is a `u64`, and `u64` is a copy type. Reading it from `borrow self` copies the value.
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
    value: i32
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
- function parameters and return types should usually be explicit
- exported public items should require explicit signatures

## 6.3 Generics

Aurora should support parametric polymorphism for containers, algorithms, channels, futures if any, and user-defined abstractions.

Example:

```python
class Cell[T]:
    value: T

def first[T](xs: Vec[T]) -> T:
    return xs[0]
```

Generics should compile efficiently through monomorphization where appropriate.

Monomorphization means the compiler generates specialized concrete code for each generic type use, such as one version of a function for `Vec[i32]` and another for `Vec[String]`.

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

## 6.5 Enums and algebraic data types

Aurora should have first-class sum types.

Example:

```python
enum Result[T, E]:
    Ok(T)
    Err(E)
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

---

# 7. Functions and methods

## 7.1 Functions

Functions use `def`.

Example:

```python
def add(x: i32, y: i32) -> i32:
    return x + y
```

## 7.2 Methods

Inherent methods are declared directly inside the `class` body.

Example:

```python
class Counter:
    value: i32 = 0

    def inc(borrow mut self):
        self.value += 1
```

The self model must make ownership and mutability clear.

Recommended receiver kinds:

- `self` for by-value
- `borrow self` for shared borrow
- `borrow mut self` for exclusive mutable borrow

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

## 7.4 Constructors

Aurora uses value constructors rather than Python-style `__init__`.

Rules:

- every class may use keyword field construction, such as `Point(x=1.0, y=2.0)`, when the relevant fields are visible
- constructor calls produce fully initialized values directly
- classes may define associated constructor methods such as `new`, `default`, `empty`, or `from_file`
- fallible constructors return `Result[Self, E]`
- partial initialization is not part of v1

Example:

```python
class User:
    public name: String
    public age: i32 = 0

    def new(name: String, age: i32 = 0) -> Self:
        return Self(name=name, age=age)

    def guest() -> Self:
        return Self(name=String("Guest"), age=0)
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
- suitable for both I/O and parallel computation

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

Example:

```python
jobs = Channel[i32](capacity=100)
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
jobs.send(42)
value = results.recv()
```

Aurora uses method-call channel operations in v1. There is no alternate operator syntax for send or receive.

## 8.4 Select

Aurora should have a `select` construct for waiting on multiple channel operations.

Example:

```python
select:
    case msg = inbox.recv():
        handle(msg)
    case sig = shutdown.recv():
        break
    case after(1s):
        heartbeat()
```

This is essential for service programming and structured concurrent systems.

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
def parse_int(s: borrow str) -> Result[i32, ParseError]:
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

## 9.3 Panics or fatal errors

Aurora may include unrecoverable failures for internal bugs or violated invariants, but these should be clearly separate from ordinary error handling.

## 9.4 Exceptions

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

A small default prelude may re-export a few extremely common names, such as `println`, while their canonical library home remains explicit modules like `fmt`.

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
    id: u64
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
    id: u64
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
def worker(id: i32, jobs: Channel[Job], out: Channel[Result[String, Error]]):
    for job in jobs:
        out.send(process(job))

def main() -> Result[None, Error]:
    jobs = Channel[Job](capacity=100)
    out = Channel[Result[String, Error]](capacity=100)

    with task_group() as group:
        for i in range(4):
            group.spawn(worker, i, jobs, out)

    return Ok(None)
```

## 17.8 Select

```python
select:
    case msg = inbox.recv():
        handle(msg)
    case err = errors.recv():
        log.error(err)
    case after(500ms):
        print("tick")
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
def distance(a: Point, b: Point) -> f64:
    ...
```

---

# 20. Implementation roadmap for Codex

This section is the concrete build plan.

## Phase 0: Decision freeze

Before writing the full compiler, freeze these decisions. The rest of this document assumes the following v1 answers:

1. Source files use the `.au` extension.
2. Package metadata lives in `Aurora.toml`.
3. Bindings use `x = 10` and `mut y = 20`; there is no `let` keyword.
4. Borrow syntax is `borrow T` and `borrow mut T`.
5. Assignment of non-copy values moves ownership.
6. Nominal product types use `class`; inherent methods live in class bodies; `impl Trait for Type` is reserved for trait conformance; classes move by default, `copy` is explicit, and recursive fields use the built-in `indirect` storage modifier.
7. Pattern matching is by value unless the scrutinee is explicitly borrowed with `match borrow value:` or `match borrow mut value:`.
8. Concurrency uses `spawn`, `task_group`, `Channel[T]`, and `select`; detached tasks require explicit `spawn detached`.
9. Recoverable errors use `Result[T, E]` and `try expr`.
10. Text uses `String` for owned values and `borrow str` for borrowed slices.
11. Visibility uses `public`, with items private by default.
12. The initial standard library uses `string`, `bytes`, `collections`, `task`, `sync`, `fs`, `fmt`, `json`, and related core modules.
13. Scoped cleanup uses `with`; there is no general-purpose `defer` in v1.

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

## Phase 5: Concurrency runtime

Implement:

- task scheduler
- channels
- select
- timers
- task spawning
- runtime shutdown behavior

Initially keep the scheduler simple but correct.

## Phase 6: Error handling and pattern matching

Implement:

- `Result`
- propagation operator
- enums
- exhaustive `match`

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

## 23.2 Borrow syntax

Aurora uses `borrow T` and `borrow mut T`.

This keeps borrowing visually explicit in an indentation-based language and reads more naturally than sigils.

## 23.3 Product type keyword

Aurora uses `class` for nominal product types.

A `class` is a value type by default. It does not imply inheritance, hidden heap allocation, or Python-style object identity.

Additional frozen rules:

- classes move by default when passed, assigned, or returned by value
- `copy` is explicit and only legal when all fields are copy types
- direct recursive class fields are illegal; recursion uses the built-in `indirect` storage modifier
- `T?` is shorthand for `Option[T]` in type positions, so recursive fields commonly look like `indirect Node?`
- accessing a non-copy field through `borrow self` does not move that field out of the object
- shared identity must be modeled through explicit wrapper types rather than plain classes

## 23.4 Trait model

Aurora uses `trait` for behavior declarations.

Inherent methods are written inside class bodies. Trait implementations use explicit `impl Trait for Type` blocks.

## 23.5 String model

Aurora uses:

- `String` for owned UTF-8 text
- `borrow str` for borrowed UTF-8 string slices

String literals have type `borrow str`. Converting borrowed text to owned text is explicit.

## 23.6 Visibility

Aurora uses `public` for exported APIs.

Items are private by default.

## 23.7 Constructor model

Aurora constructors create fully initialized values directly.

The language supports:

- keyword field construction
- associated constructor methods that return `Self`
- fallible constructors that return `Result[Self, E]`

Aurora does not use Python-style `__init__`.

## 23.8 Match ownership model

`match value:` matches by value and may move non-copy payloads.

`match borrow value:` and `match borrow mut value:` borrow the scrutinee instead.

## 23.9 Task and channel model

Aurora uses structured concurrency in v1.

- `spawn` creates a child task in the current task scope
- `task_group` owns related child tasks
- detached background work must be explicit
- channels use the `Channel[T]` type
- channel operations use methods such as `.send()`, `.recv()`, and `.close()`

## 23.10 Result placement

`Result[T, E]` is a standard-library enum with language support for `try expr`.

## 23.11 Scoped cleanup

Aurora uses `with` for scoped cleanup in v1.

There is no general-purpose `defer`.

---

# 24. Canonical language pitch

Use this as the short public description:

> **Aurora is a readable, Python-inspired compiled language with Rust-like memory safety and Go-like concurrency. It is designed for developers who want clean syntax, native performance, safe parallelism, and one modern toolchain.**

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
    x: f64
    y: f64

def distance(a: Point, b: Point) -> f64:
    dx = a.x - b.x
    dy = a.y - b.y
    return (dx * dx + dy * dy).sqrt()

def main() -> i32:
    p1 = Point(x=0.0, y=0.0)
    p2 = Point(x=3.0, y=4.0)
    println(distance(p1, p2))
    return 0
```

Once that works with parsing, type checking, compilation, and execution, move on to ownership, collections, `Result`, and tasks.

---

# 27. Closing note

Aurora should not try to be everything.

It should be one thing done well:

**a beautiful, safe, concurrent compiled language that feels familiar to Python programmers without being bound by Python’s past.**
