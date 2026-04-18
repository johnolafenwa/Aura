# Current Language Surface

This chapter is a compact reference for the language subset that the bootstrap compiler supports today.

It is intentionally implementation-facing. Use the earlier chapters to learn the language progressively, then use this chapter to check what is actually available right now.

## Top-Level Items

Aurora currently supports these top-level declarations:

- `public class`
- `public enum`
- `public def`
- `public trait`
- `public copy class`
- `class`
- `copy class`
- `enum`
- `def`
- `trait`
- `impl Trait for Type`

It also supports top-level executable statements for script-style files.

## Entry Styles

You can write either:

- a top-level script
- an explicit `main`

Do not mix top-level executable statements with `main` in the same file.

Floating-point literals default to `float64`, but they can adopt an expected `float32` type from an annotation, parameter, return type, or class field.

Integer literals now support the full `uint128` range in the checker, MIR runtime, and direct backend.

## Types

Builtin scalar and utility type names currently accepted by the compiler:

- `bool`
- `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`
- `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`
- `float32`, `float64`
- `String`
- `str` in borrowed type positions
- `None`
- `Duration`
- `Range`

Builtin generic or runtime-facing types currently accepted:

- `Option[T]`
- `Result[T, E]`
- `SendError[T]`
- `Queue[T]`
- `Vec[T]`
- `Map[K, V]`
- `Set[T]`
- `MapEntry[K, V]`
- `Task[T]`
- `TaskGroup`

These built-in type names are reserved and cannot be reused for user-defined classes, enums, or traits.

## Packages And Workspaces

Aurora now supports a first local package-system milestone:

- `Aurora.toml` package manifests with `[package]`
- package source roots under `src/`
- local path dependencies under `[dependencies]`
- git dependencies under `[dependencies]`
- workspace roots with `[workspace] members = [...]`
- package-aware `check`, `run`, `build`, `analyze`, and `complete`
- a local `Aurora.lock` written at the package root or workspace root

Current manifest shape:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
jsonx = { git = "https://github.com/example/jsonx.git", branch = "main" }
```

Current workspace shape:

```toml
[workspace]
members = ["app", "util"]
```

Current package-system limits:

- dependency imports may come from local path dependencies or git dependencies
- import roots for dependencies are package-name-prefixed, such as `import util.math`
- version-only registry dependencies like `util = "0.1.0"` are rejected with a clear diagnostic
- git dependencies support `rev`, `tag`, or `branch`, and default to `branch = "main"` when no selector is provided
- git dependencies are materialized from a local cache and pinned by exact revision in `Aurora.lock`
- `aura deps update` refreshes all branch/tag/default-main git dependencies for the current package or workspace
- `aura deps update util` refreshes just the named git dependency
- there are still no registry or publish/install flows yet

## Ownership And Borrowing

Aurora uses an ownership model with no garbage collector. See [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for the full tutorial.

Copy types (all numeric types, `bool`, `Duration`, `Queue[T]`, and `Task[T]`) are duplicated on assignment. Move types (`String`, `Vec[T]`, `Map[K, V]`, `Set[T]`, `TaskGroup`, and user-defined classes) transfer ownership on assignment.

`copy class` declarations are allowed when all fields are copy types.

Borrowing forms:

- `borrow T` -- shared, read-only parameter
- `borrow mut T` -- exclusive, mutable parameter
- `borrow self` -- shared receiver
- `borrow mut self` -- mutable receiver
- `self` -- by-value (consuming) receiver
- `for x in borrow collection:` -- shared borrow iteration
- `for x in borrow mut collection:` -- mutable borrow iteration
- `match borrow value:` -- shared borrow pattern matching
- `match borrow mut value:` -- mutable borrow pattern matching

Mutable borrow arguments must be mutable places. Overlapping `borrow mut` arguments with other borrows of the same value are rejected. Non-copy fields cannot be moved out of borrowed values.

`.clone()` produces an explicit independent copy of a move type.

## Statements

The current compiler supports these statement forms:

- assignment and compound assignment
- `return`
- `if` / `elif` / `else`
- `while`
- `for value in range(n):`
- `for value in jobs:`
- `match`
- `with`
- `select`
- `break`
- `continue`
- `pass`
- expression statements

## Expressions

The current compiler supports these expression forms:

- names
- integer, float, string, f-string, boolean, `None`, and duration literals
- arithmetic, comparison, and boolean operators
- unary prefix operators `-` and `not`
- operator-trait dispatch for `+`, binary `-`, `*`, `/`, `%`, unary `-`, and `not`
- explicit numeric casts with `expr as Type`
  - integer casts are range-checked and integer-to-float casts reject silent precision loss
- list literals such as `[1, 2, 3]`
- map literals such as `{"aurora": 1}`
- set literals such as `Set{1, 2, 3}`
- member access with `.`
- indexing with `expr[index]`
- function and method calls
- explicit type arguments on call targets such as `Box[int32](...)` and `Result[int32, String].Ok(...)`
- enum and built-in enum variant construction
- `spawn ...`
- `spawn detached ...`
- `try expr`
- parenthesized expressions

Indexed expressions remain ordinary values after parsing, so chains such as `keys[idx].clone()` and interpolations such as `f"{counts["key"]}"` are supported.

## Methods

Class methods currently support these receiver forms:

- `self`
- `borrow self`
- `borrow mut self`
- no receiver for associated methods

Ordinary functions, instance methods, and associated methods support:

- positional calls
- named arguments
- mixed calls where positional arguments come first and named arguments come after
- default parameter values on ordinary functions and class methods
- ordinary borrowed parameters with `value: borrow T` and `value: borrow mut T`
- builtin named arguments for `print(value=...)`, `range(...)`, and `after(duration=...)`

Borrowed ordinary parameters currently work for normal calls, but `spawn` and `TaskGroup.start(...)` still require by-value parameters.
Calls also reject overlapping borrowed arguments whenever a `borrow mut` parameter participates, including a `borrow mut self` receiver overlapping another borrowed argument in the same method call.
Empty list literals currently require an expected `Vec[T]` type such as `values: Vec[int32] = []`, or you can use `Vec[int32]()` explicitly.
Empty map literals currently require an expected `Map[K, V]` type such as `counts: Map[String, int32] = {}`.
Empty set literals currently require an expected `Set[T]` type such as `seen: Set[int32] = Set{}`, or you can use `Set[int32]()` explicitly.

Top-level declarations may also be generic:

- `class Box[T]: ...`
- `class Box[T: Trait]: ...`
- `enum Wrapper[T]: ...`
- `enum Wrapper[T: Trait]: ...`
- `def identity[T](value: T) -> T: ...`

Generic functions and methods may use inline trait bounds:

- `def speak[T: Greeter](value: T): ...`
- `def use_both[T: A + B](value: T) -> int32: ...`
- `def apply[T: Mapper[int32]](mapper: T, value: int32) -> int32: ...`

## Builtins

Current builtin functions:

- `print`
- `range`
- `queue`
- `tasks`
- `cancelled`
- `after`
- `sleep`
- `abs`
- `min`
- `max`
- `sqrt`
- `parse_int32`
- `parse_int64`
- `parse_float64`

Current builtin `range(...)` notes:

- supports `range(stop)` and `range(start, stop)`
- supports the matching named-argument forms
- currently requires bounds that fit the bootstrap compiler's signed index space

Current builtin member methods include:

- `float64.sqrt()`
- scalar and boolean `.to_string()`
- `String.len()`
- `String.contains(...)`
- `String.starts_with(...)`
- `String.ends_with(...)`
- `String.split(...)`
- `String.join(...)`
- `String.replace(...)`
- `String.to_lower()`
- `String.to_upper()`
- `String.strip_prefix(...)`
- `String.strip_suffix(...)`
- `String.trim()`
- `String.clone()`
- `Vec.len()`
- `Vec.is_empty()`
- `Vec.clone()`
- `Vec.push(...)`
- `Vec.pop()`
- `Vec.get(...)`
- `Vec.insert(...)`
- `Vec.set(...)`
- `Vec.remove(...)`
- `Vec.swap(...)`
- `Vec.contains(...)`
- `Vec.extend(...)`
- `Vec.clear()`
- `Vec.reverse()`
- `Map.len()`
- `Map.is_empty()`
- `Map.clone()`
- `Map.get(...)`
- `Map.set(...)`
- `Map.remove(...)`
- `Map.contains_key(...)`
- `Map.keys()`
- `Map.values()`
- `Map.items()`
- `Map.entries()`
- `Map.clear()`
- `Map.extend(...)`
- `Set.len()`
- `Set.is_empty()`
- `Set.clone()`
- `Set.contains(...)`
- `Set.insert(...)`
- `Set.remove(...)`
- `Queue.put(...)`
- `Queue.get(...)`
- `Queue.close()`
- `Task.result()`
- `TaskGroup.start(...)`
- `TaskGroup.cancel()`

## Pattern Matching

The current compiler supports:

- `Enum.Variant`
- `Enum.Variant(name)`
- multi-payload enum variants including named payload fields
- unqualified variants such as `Ok(value)` and `None` when the scrutinee type is known
- literal patterns over `bool`, integer, and `String`
- floating-point literal patterns
- `match borrow value:`
- `match borrow mut value:`
- `case _:`
- exhaustive statement-form `match`
- expression-form `match`
- nested enum patterns

Boolean literal matches are exhaustive when they cover both `true` and `false`. Integer and `String` literal matches still require a final wildcard arm.

## Concurrency

The current bootstrap concurrency surface includes:

- typed queues
- `for` iteration over queues until close
- spawned tasks
- detached tasks
- task groups
- cooperative cancellation
- `select` over send, receive, and timer arms
- duration literals with `ms`, `s`, and `m`

Current collection notes:

- `Vec.len()` returns `int32`, so `range(values.len())` works directly
- `for value in vec:`, `for value in borrow vec:`, and `for value in borrow mut vec:` are supported for `Vec[T]`
- `for value in borrow mut vec:` requires the iterable place itself to be mutable
- indexed reads no longer consume `Vec[T]` values when the element type is non-copy
- `Vec[T]` supports equality and inequality when both sides have the same `Vec[T]` type
- empty map literals still need an expected `Map[K, V]` type, or you can use `Map[K, V]()` explicitly
- `Map[K, V]` supports literal construction, indexed reads/writes, and the maintained method surface `len`, `is_empty`, `clone`, `get`, `set`, `remove`, `contains_key`, `keys`, `values`, `items`, `entries`, `clear`, and `extend`
- `Map.items()` and `Map.entries()` return `Vec[MapEntry[K, V]]`, where entry values expose `.key` and `.value`
- `Set[T]` supports literal construction with `Set{...}` and the maintained method surface `len`, `is_empty`, `clone`, `contains`, `insert`, and `remove`
- `for value in set:` and `for value in borrow set:` are supported for `Set[T]`
- `for value in borrow mut set:` is not currently supported

Timed `select` loops now treat closed receive arms as inactive when an `after(...)` arm is present, so timeout arms can still fire as an escape path. `Queue.get(timeout=...)` is also available for the ordinary single-queue timeout case.

## Tooling

The current CLI commands are:

- `check`
- `run`
- `build`
- `ast`
- `ast-json`
- `mir`
- `analyze`
- `complete`

Current backend/tooling notes:

- `build` accepts `--backend auto|direct`
- `auto` is the default
- `direct` now covers the full currently implemented Aurora language surface

The current VS Code tooling is compiler-backed for:

- diagnostics
- document symbols
- hover
- go-to-definition
- completions

## Still Outside The Bootstrap Compiler

Not yet implemented:

- non-numeric casts
- direct recursive fields without `indirect`
- broader lifetime inference beyond explicit borrowed-return sources and labels

Current module/import limitations:

- imports resolve local `.au` files relative to the current package root
- directly checking or analyzing a nested package file now infers the nearest package root that satisfies its imports
- `import a.b` exposes module namespaces for calls like `a.b.func(...)`, `a.b.Type(...)`, and `a.b.Enum.Variant`
- type annotations may use namespace-imported types such as `a.b.Type`
- the current runtimes stop with a friendly recursion-depth diagnostic after 1024 nested Aurora calls
- package manifests, local path dependencies, and git dependencies are now implemented

Current expression/ergonomics limitations:

- empty list literals still require an expected `Vec[T]` type such as `values: Vec[int32] = []`
- strings use quoted literals; `String(...)` is not a constructor
- enum variants may be called by bare built-in name when an expected type is available, for example `ok: Result[int32, String] = Ok(7)`
- `queue()` still requires an expected `Queue[T]` type annotation in the bootstrap compiler
- `queue[T]()` is supported when you want to avoid relying on expected type context
- `spawn` and `TaskGroup.start(...)` support named functions plus associated methods without `self`
- concurrency uses only the maintained `Queue[T]`, `queue()`, `Task.result()`, `tasks()`, and `TaskGroup.start(...)` surface
- borrowed return labels such as `borrow[shared]` are supported on borrowed parameters and returns for advanced zero-copy APIs
