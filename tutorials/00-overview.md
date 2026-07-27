# Overview

Aurora is a systems programming language with Python-like readability, explicit static types, and an ownership-based memory model that eliminates the need for a garbage collector.

If you know Python, Aurora will feel familiar -- same indentation-based syntax, no semicolons, `def` for functions, `class` for types. The key differences are: every variable has a known type at compile time, values have a single owner that controls their lifetime, and the language compiles to native binaries.

These tutorials teach the language as it exists in this repository today, not the full proposal surface.

## What You Can Learn Today

- top-level scripts and explicit `main`
- bindings, mutability, `None`, and the current builtin type names
- functions, owned return values, typed parameters, and shared or mutable access
- classes, keyword construction, defaults, receivers, and methods
- ownership, borrowing, move semantics, copy types, and cloning
- owned `Vec[T]`, `Map[K, V]`, and `Set[T]` collections with literals, indexing, and iteration
- enums, exhaustive `match`, built-in `Result[T, E]`, `Option[T]`, and `SendError[T]`
- strings, string parsing/formatting, numbers, signed computed Duration values, and the current builtin methods
- `if`, `elif`, `else`, `while`, `for range(...)`, `break`, and `continue`
- statement-form `match` over enum variants plus literal `bool`, integer, and `String` cases
- `with`, `try expr`, queues, structured task groups, task waiting helpers, and task timeouts
- expression-form `match`, nested enum patterns, and multi-payload variants
- owned returns, including ordinary copies and explicit non-copy clones or transfers
- user-defined generic classes, enums, and functions
- trait declarations, trait impls, and bounded generic calls
- local file modules with `import`, `from ... import ...`, and `public` visibility
- `Aurora.toml` packages with local path dependencies, git dependencies, and workspaces
- CLI inspection commands and compiler-backed editor tooling

## What The Bootstrap Compiler Currently Supports

Today's working subset includes:

- `class`, `enum`, and `def`
- `trait` plus `impl Trait for Type`
- top-level executable statements
- explicit type annotations and inferred bindings
- mutable reassignment with `mut`
- omitted `-> None` return types
- ownership and borrowing with `T` and `mut T`
- user-defined enums plus built-in `Result`, `Option`, and `SendError`
- user-defined generic classes, enums, and functions
- builtin `Vec[T]`, `Map[K, V]`, and `Set[T]` collections with literals
- class methods with shared `self`, consuming `own self`, and mutable `mut self`
- arithmetic, comparisons, strings, booleans, and Duration literals, constructors, conversions, and checked operators
- `if`, `elif`, `else`, `while`, `for`, `match`, `with`, `break`, and `continue`
- `print`, `range`, `cancelled`, `sleep`, `wait_any`, and `wait_all`
- machine-readable compiler output for AST, analysis, and completions

## What Is Still Outside The Bootstrap Surface

The repository is not at the full proposal yet. Notable gaps include:

- full dependency registries and version solving beyond local/git package dependencies
- any future first-class loan or view values, whose design is not reserved by
  the current return syntax
- further direct-backend hardening and the remaining coverage push toward 100%

## Recommended Companion Material

Keep the `examples/` tree open while reading. The categorized examples mirror these chapters and stay runnable as the language evolves.

If you are coming from Python, the single most important chapter is [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md). It explains how Aurora manages memory without a garbage collector and shows you how to fix every common compiler error you will encounter.
