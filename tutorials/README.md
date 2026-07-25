# Aurora Tutorials

This directory is the beginning of the Aurora tutorial track: a book-style set of Markdown chapters that explains the language as it exists in the repository today.

These tutorials are intentionally scoped to the implemented subset of Aurora. They should stay in sync with the compiler, the examples, and the normative Manual. The original language proposal is historical design material, not the source of current behavior.

## Maintenance Rule

When the implemented language surface changes, update these in the same pass:

1. the relevant tutorial chapter
2. the relevant example program under `examples/`
3. any CLI or tooling docs that reference the changed behavior
4. `14-current-language-surface.md` if the supported surface changed

## Reading Order

1. [00-overview.md](00-overview.md)
2. [01-running-programs.md](01-running-programs.md)
3. [02-bindings-and-types.md](02-bindings-and-types.md)
4. [03-functions.md](03-functions.md)
5. [04-control-flow.md](04-control-flow.md)
6. [05-classes-and-data.md](05-classes-and-data.md)
7. [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md)
8. [07-strings-and-numbers.md](07-strings-and-numbers.md)
9. [08-tooling.md](08-tooling.md)
10. [09-enums-and-match.md](09-enums-and-match.md)
11. [10-results-and-options.md](10-results-and-options.md)
12. [11-resource-management.md](11-resource-management.md)
13. [12-error-propagation.md](12-error-propagation.md)
14. [13-concurrency.md](13-concurrency.md)
15. [14-current-language-surface.md](14-current-language-surface.md)
16. [15-generics.md](15-generics.md)
17. [16-traits.md](16-traits.md)
18. [17-modules-and-visibility.md](17-modules-and-visibility.md)
19. [18-packages-and-workspaces.md](18-packages-and-workspaces.md)
20. [19-io-and-networking.md](19-io-and-networking.md)
21. [20-randomness.md](20-randomness.md)
22. [21-json.md](21-json.md)
23. [22-bytes.md](22-bytes.md)
24. [23-assertions.md](23-assertions.md)
25. [24-multiline-expressions.md](24-multiline-expressions.md)
26. [25-tuples.md](25-tuples.md)

## Scope Today

The current tutorial set covers:

- scripts and `main`
- bindings, mutability, and type annotations
- functions with explicit and omitted `None` return types
- classes with fields, default values, receiver forms, mutating methods, and `public` field syntax
- ownership, declaration-stable parameter defaults, explicit `own`, move
  semantics, copy types, and the exclusivity rule for mutable borrows
- owned `Vec[T]`, `Map[K, V]`, and `Set[T]` collections with literals,
  storing APIs, shared-default/`own` iteration, and mutable Vec iteration
- enums with exhaustive `match`
- user-defined generic classes, enums, and functions
- trait declarations, trait impls, and bounded generic calls
- local file modules with `import`, `from ... import ...`, and `public` visibility at module boundaries
- `Aurora.toml` packages with `src/`, local path dependencies, git dependencies, workspaces, and local lockfiles
- built-in `Result[T, E]`, `Option[T]`, `SendError[T]`, and bare `None`
- `try expr`
- conditional expressions such as `value if condition else alternative`, with
  exact-`bool` conditions and lazy selection of one arm
- `in` and `not in` over `Vec`, `Set`, `Map` keys, and `String` substrings
- Python-style chained comparisons such as `low <= value < high`, which
  evaluate each operand once and short-circuit
- `with` using `close(borrow mut self)` and `with TaskGroup() as group:`
- builtin `io`, `fs`, `net`, and `process` modules with scheduler-aware file I/O, maintained networking resource types, and shell-free subprocess helpers
- `Queue[T]()`, `Task[T].result()`, `TaskGroup()`, `TaskGroup.start(...)`, `TaskGroup.start_soon(...)`, `wait_any(...)`, `wait_all(...)`, send-result errors, and cooperative cancellation
- arithmetic including explicit floor division, integer-to-float conversion, and computed signed Duration values; strings, string parsing/formatting, booleans, and comparisons
- deterministic seeded randomness, unbiased ranges, mutable-Vec shuffle, and
  the separate OS-secure integer/byte boundary
- recursive `json.Value` trees, typed parse errors, exact accessors, consuming
  payload extraction, and deterministic compact or pretty dumping
- `Vec[uint8]` bytes, strict UTF-8 conversion, canonical hex/base64 codecs,
  typed malformed-input errors, and raw SHA-256
- `assert condition` and `assert condition, message`, with lazy messages,
  source-located `AU4001` failures, and file-level `aura test` behavior
- delimiter-based newline continuation inside `()`, `[]`, and `{}`, including
  multiline signatures, calls, grouping, indexing, and collection literals;
  ordinary trailing commas, backslash continuation, and multiline f-strings
  remain unavailable (singleton tuples require their one comma)
- fixed structural tuples with parenthesized value/type syntax, recursive
  assignment/loop unpacking and patterns, whole-source move behavior, and
  copy-only constant indexing
- `if`, `elif`, `else`, `for`, `while`, `match`, `break`, and `continue`
- `print`
- CLI inspection commands such as `check`, `ast`, `ast-json`, `analyze`, `complete`, and `mir`
- compiler-backed VS Code diagnostics, navigation, and completions

Use the normative [Language Specification](../docs/manual/language-specification.md) and [Manual](../docs/manual/index.md) as the exhaustive truth source. `14-current-language-surface.md` is a compact tutorial recap; the earlier chapters should explain the maintained surface progressively.

It does not yet attempt to teach features that are still only in the proposal.
