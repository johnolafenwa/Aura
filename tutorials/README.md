# Aurora Tutorials

This directory is the beginning of the Aurora tutorial track: a book-style set of Markdown chapters that explains the language as it exists in the repository today.

These tutorials are intentionally scoped to the implemented subset of Aurora. They should stay in sync with the compiler, the examples, and the language proposal.

## Maintenance Rule

When the implemented language surface changes, update these in the same pass:

1. the relevant tutorial chapter
2. the relevant example program under `examples/`
3. any CLI or tooling docs that reference the changed behavior
4. `13-current-language-surface.md` if the supported surface changed

## Reading Order

1. [00-overview.md](00-overview.md)
2. [01-running-programs.md](01-running-programs.md)
3. [02-bindings-and-types.md](02-bindings-and-types.md)
4. [03-functions.md](03-functions.md)
5. [04-control-flow.md](04-control-flow.md)
6. [05-classes-and-data.md](05-classes-and-data.md)
7. [06-strings-and-numbers.md](06-strings-and-numbers.md)
8. [07-tooling.md](07-tooling.md)
9. [08-enums-and-match.md](08-enums-and-match.md)
10. [09-results-and-options.md](09-results-and-options.md)
11. [10-resource-management.md](10-resource-management.md)
12. [11-error-propagation.md](11-error-propagation.md)
13. [12-concurrency.md](12-concurrency.md)
14. [13-current-language-surface.md](13-current-language-surface.md)
15. [14-generics.md](14-generics.md)
16. [15-traits.md](15-traits.md)
17. [16-modules-and-visibility.md](16-modules-and-visibility.md)

## Scope Today

The current tutorial set covers:

- scripts and `main`
- bindings, mutability, and type annotations
- functions with explicit and omitted `None` return types
- classes with fields, default values, receiver forms, mutating methods, and `public` field syntax
- owned `Vec[T]`, `Map[K, V]`, and `Set[T]` collections with literals, methods, and iteration where supported
- enums with exhaustive `match`
- user-defined generic classes, enums, and functions
- trait declarations, trait impls, and bounded generic calls
- local file modules with `import`, `from ... import ...`, and `public` visibility at module boundaries
- built-in `Result[T, E]`, `Option[T]`, `SendError[T]`, and bare `None`
- `try expr`
- `with` using `close(borrow mut self)` and `with task_group() as group:`
- `Channel[T]`, `channel()`, `spawn`, `spawn detached`, `Task[T].join()`, `task_group()`, `select`, send-result errors, and cooperative cancellation
- arithmetic, strings, string parsing/formatting, booleans, comparisons, and duration literals
- `if`, `elif`, `else`, `for`, `while`, `match`, `break`, and `continue`
- `print`
- CLI inspection commands such as `check`, `ast`, `ast-json`, `analyze`, `complete`, and `mir`
- compiler-backed VS Code diagnostics, navigation, and completions

Use `13-current-language-surface.md` as the compact truth source for the currently implemented subset. The earlier chapters should explain that surface progressively, but that reference chapter should stay exhaustive for the bootstrap compiler.

It does not yet attempt to teach features that are still only in the proposal, such as detached-task ownership restrictions or the fuller network/runtime model beyond the current bootstrap runtime.
