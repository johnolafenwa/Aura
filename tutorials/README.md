# Aura Tutorials

These chapters teach Aura as the compiler in this repository implements it
today. Read them in order, like a book. Each chapter introduces one part of
the language and shows runnable code.

The tutorials cover only implemented features. They do not teach anything
that exists only in the language proposal. For the exhaustive rules, use the
normative [Language Specification](../docs/manual/language-specification.md)
and [Manual](../docs/manual/index.md).

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
24. [23-assertions-and-tests.md](23-assertions-and-tests.md)
25. [24-multiline-expressions.md](24-multiline-expressions.md)
26. [25-tuples.md](25-tuples.md)
27. [26-ffi.md](26-ffi.md)

`14-current-language-surface.md` is a compact recap of the whole surface. The
other chapters build it up step by step.

## Scope Today

The tutorial set covers the topics below.

### Programs And Syntax

- scripts and `main`
- bindings, mutability, and type annotations
- `if`, `elif`, `else`, `for`, `while`, `match`, `break`, and `continue`
- `print`
- conditional expressions such as `value if condition else alternative`.
  The condition must be exactly `bool`, and only the selected arm is
  evaluated.
- `in` and `not in` over `list`, `set`, `dict` keys, and `str` substrings
- Python-style chained comparisons such as `low <= value < high`. Each operand
  is evaluated once, and the chain short-circuits.
- the loop forms `for ... in enumerate(seq):` and
  `for ... in zip(first, second):`. `zip` stops at the shorter sequence.
- newline continuation inside `()`, `[]`, and `{}`. This covers multiline
  signatures, calls, grouping, indexing, and collection literals. Ordinary
  trailing commas, backslash continuation, and multiline f-strings are not
  available. A singleton tuple still needs its one comma.

### Functions And Closures

- functions with explicit and omitted `None` return types
- capture-free named function values with `def(T1, mut T2, own T3) -> R`
  types. This includes copy and `Transfer` semantics, indirect calls, storage,
  and task targets.
- contextually typed expression lambdas. They support by-value Copy and
  non-Copy capture, exhaustive shared, mutable, and owned capture lists,
  repeatable shared and mutable calls, consuming single-use calls, and
  structural Transfer.

### Types And Data

- classes with fields, default values, receiver forms, mutating methods, and
  `public` field syntax
- enums with exhaustive `match`
- user-defined generic classes, enums, and functions
- trait declarations, trait impls, and bounded generic calls
- built-in `Result[T, E]`, optional `T | None` unions, `Lookup[T]`,
  `Poll[T]`, `SendError[T]`, and bare `None`
- fixed structural tuples:
  - parenthesized value and type syntax
  - recursive assignment and loop unpacking, and tuple patterns
  - whole-source move behavior
  - copy-only constant indexing
  - same-type recursive `==` and `!=`, which keep both operands
  - tuple ordering is not available

### Ownership

- ownership and move semantics
- declaration-stable parameter defaults and explicit `own`
- copy types
- place-based shared and mutable views, returned views, and reborrowing
- inferred loan lifetimes and exclusivity

### Collections And Arrays

- owned `list[T]`, `dict[K, V]`, and `set[T]` collections:
  - literals and storing APIs
  - bare-shared and `own` iteration, and mutable list iteration
  - stable sorting
  - eager callback-powered `map` and `filter`
  - eager owned list, set, and dictionary comprehensions with filters and
    nested clauses
- owned list and str slices with omitted endpoints, negative normalization,
  loud bounds, and Unicode-scalar str positions
- owned contiguous numeric `Array[T]` values for the four maintained dtypes:
  - exact shapes and row-major multidimensional indexing
  - first-axis copy slices
  - same-dtype arithmetic, with explicit wrapping and saturating integer modes
  - mapping and deterministic reductions

### Numbers And Strings

- arithmetic, including:
  - decimal, hexadecimal, binary, and octal integer literals
  - fixed-width bitwise and shift operations
  - checked power, ties-to-even `round`, and paired `divmod`
  - explicit floor division and integer-to-float conversion
  - computed signed Duration values
- strings, string parsing and formatting, booleans, and comparisons
- the builtin functions `len(value)`, which delegates to the value's own
  `len()`, and `str(value)`, which produces the print rendering.
  `str.len()`, `str.byte_len()`, `list.len()`, `dict.len()`, and `set.len()`
  all return `int64`. That matches range bounds, list indexes, slice
  endpoints, enumeration positions, and Array coordinates.

### Errors And Resources

- `try expr`
- `with` using `close(mut self)`, and `with TaskGroup() as group:`
- `assert condition` and `assert condition, message`, with lazy messages,
  source-located `AU4001` failures, and file-level `aura test` behavior

### Modules And Packages

- local file modules with `import`, `from ... import ...`, and `public`
  visibility at module boundaries
- `Aura.toml` packages with `src/`, local path dependencies, git
  dependencies, workspaces, and local lockfiles
- package-authorized FFI v0 with bodyless `extern "C"` declarations,
  fixed-width scalars, pointer-length str and byte views, opaque handles, and
  exact root dependency reports

### Concurrency

- `Queue[T]()`, `Task[T].result()`, and `TaskGroup()` with its ordinary and
  explicit-stack start methods
- typed `select(...)` over Queue, Task, and relative-Duration sources
- `wait_any(...)` and `wait_all(...)`
- send-result errors and structural `Transfer` boundaries
- single-consumer task results and cooperative cancellation

### Standard Library

- builtin `io`, `fs`, `net`, and `process` modules, with scheduler-aware file
  I/O, maintained networking resource types, and shell-free subprocess helpers
- deterministic seeded randomness, unbiased ranges, and mutable-list shuffle,
  plus the separate OS-secure integer and byte boundary
- `control.retry` for eager `Result` workers, with an attempt budget and
  exponential `Duration` backoff
- recursive `json.Value` trees, typed parse errors, exact accessors,
  consuming payload extraction, and deterministic compact or pretty dumping
- `list[uint8]` bytes, strict UTF-8 conversion, canonical hex and base64
  codecs, typed malformed-input errors, and raw SHA-256

### Tooling

- CLI inspection commands such as `check`, `ast`, `ast-json`, `analyze`,
  `complete`, and `mir`
- compiler-backed VS Code diagnostics, navigation, and completions

## Maintenance Rule

When the implemented language surface changes, update these in the same pass:

1. the relevant tutorial chapter
2. the relevant example program under `examples/`
3. any CLI or tooling docs that describe the changed behavior
4. `14-current-language-surface.md`, if the supported surface changed
