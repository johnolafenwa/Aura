# ADR-0004: String semantics

- Status: Accepted
- Date: 2026-07-13
- Amended: 2026-07-26 (B3.0-d int64 String length results); 2026-07-31
  (ADR-0040 owned scalar slices)
- Roadmap decision: D4

## Decision

`String.len() -> int64` returns the number of Unicode scalar values and
therefore runs in O(n). `String.byte_len() -> int64` returns the number of
bytes in the UTF-8 encoding and runs in O(1). The B3.0-d amendment aligns both
results with the maintained `int64` length surface; it does not change either
counting rule.

Ordinary string literals may use matching single or double quote delimiters.
Both forms decode the same escape set, including `\"` and `\'`. F-strings
remain double-quoted as `f"..."`; this decision does not add `f'...'`, character
literals, triple-quoted strings, raw strings, or byte-string literals.

Integer indexing on `String` remains unavailable. Under ADR-0040,
`text[start:end]`, `text[:end]`, `text[start:]`, and `text[:]` return fresh
owned Strings. Written endpoints have exactly type `int32`, count Unicode
scalar values, and use the negative normalization and loud bounds policy
below. String slicing is O(n) in the source text and never produces a view.
`chars()`, `ord()`, and `chr()` remain unavailable. Exact UTF-8 conversion is
provided separately by the maintained String/bytes surface.

Negative indexing is the language-wide policy for `Vec` indexing and owned
Vec/String slices.
For direct `[]` reads and writes and for `get`, `set`, `remove`, `swap`, and
`insert`, a negative index `i` is normalized once as `len + i`. The operation
then applies its existing bounds contract: direct reads/writes and mutating
methods trap when the normalized index is invalid, while `get` returns `None`.
The valid insertion range remains `0..=len`, so `insert(-1, value)` inserts
before the last element and `insert(len, value)` appends.

Unlike Python, Aurora does not clamp an insertion index that remains out of
range after normalization. Clamping can silently place a value at the wrong
position; Aurora treats that as a broken invariant and reports a runtime error.
ADR-0040 applies the same rule to Vec and String slice endpoints: both
normalized endpoints must be in `0..=len`, and a start greater than its end is
also a runtime error rather than an empty or clamped slice.

## Completion tests

- Lexer escape/span tests and single-quoted parse/run fixtures.
- String builtin unit tests in MIR and native runtimes, including the `int64`
  result types for scalar and UTF-8 byte counts.
- Vec negative-index check/run fixtures on both forced backends.
- Vec and Unicode-scalar String slice fixtures under ADR-0040, including
  omitted and negative endpoints, loud bounds failures, and owned-result
  independence.
- API/reference, tutorial, example, and LSP completion coverage.
