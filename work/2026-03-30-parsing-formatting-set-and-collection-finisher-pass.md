# 2026-03-30 Parsing, Formatting, Set, And Collection Finishers

## Goal

Land the next practical-programming milestone in one pass by adding:

- string parsing and formatting helpers
- the next `Vec[T]` finisher methods
- the next `Map[K, V]` ergonomics surface
- a first maintained `Set[T]` collection

## Work Completed

- Added builtin parsing functions `parse_int32`, `parse_int64`, and `parse_float64`, plus scalar and boolean `.to_string()` support across checking, interpreter execution, MIR lowering/runtime, direct native builds, compiler-backed analysis, and JS fallback analysis.
- Added `String.join(...)` on separator strings so `Vec[String]` values can be formatted directly into text.
- Added `Vec.insert(...)`, `Vec.clear()`, and `Vec.reverse()` across the full compiler/runtime/backend/tooling stack.
- Added `Map.items()`, `Map.entries()`, `Map.clear()`, and `Map.extend(...)`, together with builtin `MapEntry[K, V]` field access via `.key` and `.value`.
- Added owned `Set[T]` support with `Set{...}` literals, `Set[T]()` construction, by-value/shared-borrow iteration, and the maintained method surface `len`, `is_empty`, `clone`, `contains`, `insert`, and `remove`.
- Added fixture coverage for the new maintained surface, including set literal diagnostics and runtime behavior.
- Added compiler example smoke coverage plus CLI product coverage for the new maintained examples:
  - `examples/collections/set_basics.au`
  - `examples/strings/string_parsing_and_formatting.au`
- Expanded the maintained examples, tutorials, READMEs, and example index so they describe the implemented surface instead of the earlier smaller subset.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- The next practical-language milestone should focus on higher-level data-processing ergonomics instead of more backend work, for example `String` parsing helpers beyond the current scalar set, richer `Map[K, V]` iteration ergonomics, or a dedicated `Set[T]` polish pass if real examples expose rough edges.
