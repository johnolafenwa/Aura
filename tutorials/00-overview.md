# Overview

Aura is a compiled, statically typed programming language with Python-inspired
syntax, explicit ownership, native executables, and no garbage collector.

If you know Python, Aura will feel familiar: indentation defines blocks,
functions use `def`, classes use `class`, and semicolons are unnecessary. The
compiler assigns every expression a type, checks how values and resources are
owned, and validates the program before execution.

Aura 0.3 focuses on reliable applications, agents, and ML infrastructure. The
long-term goal is a general-purpose systems language capable of building the
full software stack, including operating systems and device drivers.

These tutorials teach the language as it exists in this repository today, not the full proposal surface.

## What You Can Learn Today

- top-level scripts and explicit `main`
- bindings, mutability, `None`, and the current builtin type names
- functions, owned return values, typed parameters, and shared or mutable access
- classes, keyword construction, defaults, receivers, and methods
- ownership, borrowing, move semantics, copy types, and cloning
- owned `list[T]`, `dict[K, V]`, and `set[T]` collections with literals, indexing, and iteration
- enums, exhaustive `match`, built-in `Result[T, E]`, `Option[T]`, and `SendError[T]`
- strings, string parsing/formatting, numbers, signed computed Duration values, and the current builtin methods
- `if`, `elif`, `else`, `while`, `for range(...)`, `break`, and `continue`
- statement-form `match` over enum variants plus literal `bool`, integer, and `str` cases
- `with`, `try expr`, queues, structured task groups, task waiting helpers, and task timeouts
- expression-form `match`, nested enum patterns, and multi-payload variants
- owned returns, including ordinary copies and explicit non-copy clones or transfers
- user-defined generic classes, enums, and functions
- trait declarations, trait impls, and bounded generic calls
- local file modules with `import`, `from ... import ...`, and `public` visibility
- `Aura.toml` packages with local path dependencies, git dependencies, and workspaces
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
- builtin `list[T]`, `dict[K, V]`, and `set[T]` collections with literals
- class methods with shared `self`, consuming `own self`, and mutable `mut self`
- arithmetic, comparisons, strings, booleans, and Duration literals, constructors, conversions, and checked operators
- `if`, `elif`, `else`, `while`, `for`, `match`, `with`, `break`, and `continue`
- `print`, `range`, `cancelled`, `sleep`, `wait_any`, and `wait_all`
- machine-readable compiler output for AST, analysis, and completions

## Current Boundaries

Notable limits include:

- full dependency registries and version solving beyond local/git package dependencies
- further direct-backend hardening and the remaining coverage push toward 100%

## Recommended Companion Material

Keep the `examples/` tree open while reading. The categorized examples mirror these chapters and stay runnable as the language evolves.

If you are coming from Python, the single most important chapter is [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md). It explains how Aura manages memory without a garbage collector and shows you how to fix every common compiler error you will encounter.
