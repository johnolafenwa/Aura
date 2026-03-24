# Overview

Aurora is aiming for Python-like readability with explicit static types, ownership-oriented design, and systems-language tooling.

These tutorials teach the language as it exists in this repository today, not the full proposal surface.

## What You Can Learn Today

- top-level scripts and explicit `main`
- bindings, mutability, `None`, and the current builtin type names
- functions, return rules, and typed parameters
- classes, keyword construction, defaults, receivers, and methods
- enums, exhaustive `match`, built-in `Result[T, E]`, `Option[T]`, and `SendError[T]`
- strings, numbers, duration literals, and the current builtin methods
- `if`, `elif`, `else`, `while`, `for range(...)`, `break`, and `continue`
- `with`, `try expr`, channels, spawned tasks, detached tasks, task groups, and `select`
- CLI inspection commands and compiler-backed editor tooling

## What The Bootstrap Compiler Currently Supports

Today’s working subset includes:

- `class`, `enum`, and `def`
- `trait` plus `impl Trait for Type`
- top-level executable statements
- explicit type annotations and inferred bindings
- mutable reassignment with `mut`
- omitted `-> None` return types
- user-defined enums plus built-in `Result`, `Option`, and `SendError`
- user-defined generic classes, enums, and functions
- class methods with `self`, `borrow self`, and `borrow mut self`
- arithmetic, comparisons, strings, booleans, and duration literals
- `if`, `elif`, `else`, `while`, `for`, `match`, `with`, `select`, `break`, and `continue`
- `print`, `range`, `channel`, `task_group`, `cancelled`, `after`, and `sleep`
- machine-readable compiler output for AST, analysis, and completions

## What Is Still Outside The Bootstrap Surface

The repository is not at the full proposal yet. Notable gaps include:

- nested match patterns
- expression-form `match`
- borrowed return types and explicit lifetime syntax

## Recommended Companion Material

Keep the `examples/` tree open while reading. The categorized examples are meant to mirror these chapters and stay runnable as the language evolves.
