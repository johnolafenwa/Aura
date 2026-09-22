# Overview

These tutorials teach Aura as the compiler in this repository implements it
today. They do not teach proposed features that are not built yet.

Aura is a compiled, statically typed language with Python-inspired syntax. It
has explicit ownership, builds native executables, and has no garbage
collector.

If you know Python, Aura will look familiar. Indentation defines blocks,
functions use `def`, classes use `class`, and lines need no semicolons. Before
a program runs, the compiler gives every expression a type and checks how
values and resources are owned.

Aura 0.3 focuses on reliable applications, agents, and ML infrastructure. The
long-term goal is a general-purpose systems language that can build the full
software stack, including operating systems and device drivers.

## What You Can Learn Today

- top-level scripts and explicit `main`
- bindings, mutability, `None`, and the builtin type names
- functions, owned return values, typed parameters, and shared or mutable access
- classes, keyword construction, defaults, receivers, and methods
- ownership, borrowing, move semantics, copy types, and cloning
- owned `list[T]`, `dict[K, V]`, and `set[T]` collections with literals, indexing, and iteration
- enums, exhaustive `match`, built-in `Result[T, E]`, optional `T | None` unions, `Lookup[T]`, and `SendError[T]`
- strings, string parsing and formatting, numbers, signed computed `Duration` values, and the builtin methods
- `if`, `elif`, `else`, `while`, `for range(...)`, `break`, and `continue`
- statement-form `match` over enum variants and over literal `bool`, integer, and `str` cases
- `with`, `try expr`, queues, structured task groups, task waiting helpers, and task timeouts
- expression-form `match`, nested enum patterns, and multi-payload variants
- owned returns, including ordinary copies and explicit non-copy clones or transfers
- user-defined generic classes, enums, and functions
- trait declarations, trait impls, and bounded generic calls
- local file modules with `import`, `from ... import ...`, and `public` visibility
- `Aura.toml` packages with local path dependencies, git dependencies, and workspaces
- CLI inspection commands and compiler-backed editor tooling

## What The Bootstrap Compiler Currently Supports

The bootstrap compiler is the compiler in this repository. Its working subset
is the list above. These details fill in the parts that list names only in
passing:

- `class`, `enum`, `def`, `trait`, and `impl Trait for Type`
- top-level executable statements
- explicit type annotations and inferred bindings
- mutable reassignment with `mut`
- functions that omit `-> None` from their return type
- ownership and borrowing with `T` and `mut T`
- class methods with shared `self`, consuming `own self`, and mutable `mut self`
- user-defined enums, optional `T | None` unions, and the built-in `Result`,
  `Lookup`, `Poll`, and `SendError` types
- builtin `list[T]`, `dict[K, V]`, and `set[T]` collections with literals
- arithmetic, comparisons, strings, and booleans
- `Duration` literals, constructors, conversions, and checked operators
- `if`, `elif`, `else`, `while`, `for`, `match`, `with`, `break`, and `continue`
- the builtins `print`, `range`, `cancelled`, `sleep`, `wait_any`, and `wait_all`
- machine-readable compiler output for the syntax tree, analysis, and completions

## Current Boundaries

Notable limits:

- Packages can depend on local paths and git repositories. There is no full
  package registry and no version solving.
- The direct native backend is still being hardened.
- Test coverage has not yet reached 100%.

## Recommended Companion Material

Keep the `examples/` tree open while you read. Its categories mirror these
chapters, and every example stays runnable.

If you come from Python, the most important chapter is
[06-ownership-and-borrowing.md](06-ownership-and-borrowing.md). It explains
how Aura manages memory without a garbage collector. It also shows how to fix
each common compiler error you will meet.
