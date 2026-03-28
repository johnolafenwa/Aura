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

Integer literals now support the full `uint128` range in the checker, interpreter, MIR runtime, and direct backend.

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
- `Channel[T]`
- `Task[T]`
- `TaskGroup`

These built-in type names are reserved and cannot be reused for user-defined classes, enums, or traits.

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
- explicit numeric casts with `expr as Type`
- member access with `.`
- function and method calls
- explicit type arguments on call targets such as `Box[int32](...)` and `Result[int32, String].Ok(...)`
- enum and built-in enum variant construction
- `spawn ...`
- `spawn detached ...`
- `try expr`
- parenthesized expressions

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

Borrowed ordinary parameters currently work for normal calls, but `spawn` and `TaskGroup.spawn(...)` still require by-value parameters.

Top-level declarations may also be generic:

- `class Box[T]: ...`
- `class Box[T: Trait]: ...`
- `enum Wrapper[T]: ...`
- `enum Wrapper[T: Trait]: ...`
- `def identity[T](value: T) -> T: ...`

Generic functions and methods may use inline trait bounds:

- `def speak[T: Greeter](value: T): ...`
- `def use_both[T: A + B](value: T) -> int32: ...`

## Builtins

Current builtin functions:

- `print`
- `range`
- `channel`
- `task_group`
- `cancelled`
- `after`
- `sleep`

Current builtin `range(...)` notes:

- supports `range(stop)` and `range(start, stop)`
- supports the matching named-argument forms
- currently requires bounds that fit the bootstrap compiler's signed index space

Current builtin member methods include:

- `float64.sqrt()`
- `String.clone()`
- `Channel.clone()`
- `Channel.send(...)`
- `Channel.recv()`
- `Channel.close()`
- `Task.clone()`
- `Task.join()`
- `TaskGroup.spawn(...)`
- `TaskGroup.cancel()`

## Pattern Matching

The current compiler supports:

- `Enum.Variant`
- `Enum.Variant(name)`
- unqualified variants such as `Ok(value)` and `None` when the scrutinee type is known
- `match borrow value:`
- `match borrow mut value:`
- `case _:`
- exhaustive statement-form `match`

It does not yet support expression-form `match`, but ordinary nested `match` statements are supported.

## Concurrency

The current bootstrap concurrency surface includes:

- typed channels
- `for` iteration over channels until close
- spawned tasks
- detached tasks
- task groups
- cooperative cancellation
- `select` over send, receive, and timer arms
- duration literals with `ms`, `s`, and `m`

## Tooling

The current CLI commands are:

- `check`
- `run`
- `run-mir`
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

- positional class constructor arguments
- keyword arguments for enum variant payloads
- non-numeric casts
- direct recursive fields without `indirect`
- borrowed return types and explicit lifetime syntax

Current module/import limitations:

- imports resolve local `.au` files relative to the current package root
- `import a.b` exposes module namespaces for calls like `a.b.func(...)`, `a.b.Type(...)`, and `a.b.Enum.Variant`
- type annotations may use namespace-imported types such as `a.b.Type`
- the current runtimes stop with a friendly recursion-depth diagnostic after 1024 nested Aurora calls
- package manifests and external dependencies are still proposal-only
