# 2026-03-29 String Builtins And Map Collections

## Goal

Fill the next practical-programming gap in the maintained Aurora surface by adding:

- richer builtin `String` methods
- first-pass numeric helper builtins
- an owned `Map[K, V]` collection type

## Work Completed

- Added `String.len()`, `String.contains(...)`, `String.starts_with(...)`, `String.ends_with(...)`, and `String.trim()` across checking, interpreter execution, MIR lowering/runtime, direct native builds, compiler-backed analysis, and JS fallback analysis.
- Added builtin numeric helpers `abs(...)`, `min(...)`, `max(...)`, and `sqrt(...)` across the same execution and tooling paths.
- Added owned `Map[K, V]` support with literals, `Map[K, V]()` construction, indexed reads/writes, and the maintained method surface `len`, `is_empty`, `clone`, `get`, `set`, `remove`, `contains_key`, `keys`, and `values`.
- Added new compiler fixtures for the new maintained surface, including map literal diagnostics and runtime examples.
- Added CLI regression coverage for direct-build and `run-mir` parity over the new String/number/Map surface.
- Extended compiler-backed analysis and LSP fallback/bridge tests so completions now expose the new `String` and `Map` members plus the new builtin numeric functions.
- Added maintained runnable examples:
  - `examples/strings/string_methods.au`
  - `examples/numbers/numeric_builtins.au`
  - `examples/collections/map_basics.au`
- Updated tutorials, READMEs, the example index, and the task board to document the new maintained language surface.

## Verification

- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- Add the next practical-library pass on top of this surface instead of expanding more execution paths first.
- Consider `Map[K, V]` follow-on ergonomics separately if the maintained examples start needing insertion-order or higher-order helper methods.
