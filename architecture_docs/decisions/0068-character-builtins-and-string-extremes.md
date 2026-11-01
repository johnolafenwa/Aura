# ADR-0068: Character builtins and string `min`/`max`

- Status: Accepted
- Date: 2026-11-01
- Builds on: ADR-0063, ADR-0065, and ADR-0066

## Context

After ADR-0066, the Python gaps left in everyday text code were `ord`,
`chr`, iterating a string's characters, and `min`/`max` of strings. Aura
has no character type and no integer string indexing, so code that shifts
letters or counts character codes had no route at all. ADR-0065 limited
`min` and `max` to numbers. John approved closing these gaps on 2026-11-01.

## Decision

| Form | Operands | Result |
| --- | --- | --- |
| `ord(text)` | a `str` | `int64`, the Unicode scalar value of its only character |
| `chr(code)` | an `int64` | a one-character `str` |
| `text.chars()` | a `str` receiver | `list[str]` of one-character strings, in order |
| `min(a, b, ...)`, `max(a, b, ...)` | two or more `str` values | a copy of the first smallest or largest |
| `min(values)`, `max(values)` | a `list[str]`, `set[str]`, or `Span[str]` | a copy of the first smallest or largest element |

- A character stays a one-character `str`. There is no new character type.
- `ord` of a string whose length is not one, and `chr` of a negative code,
  a surrogate (`0xD800` to `0xDFFF`), or a code above `0x10FFFF`, stop the
  program with `AU4003`, the code other invalid text-method arguments use.
  Python raises an exception for the same inputs; Aura has no exceptions for
  this, and a `Result` would make the common case noisy.
- Strings order by `<`, which compares UTF-8 bytes, so scalar-value order.
  Ties keep the first value, as numbers do. The arguments and the collection
  are read through shared access and stay usable; the result is a fresh
  copy, because `str` is not a Copy type.
- An empty `str` collection stops with `AU4001`, as an empty numeric one does.
- `sum` stays numeric. `Array` has no `str` or `bool` dtype, so nothing
  changes for it.
- `ord` and `chr` join the reserved builtin names (`AU2007`), which is the
  one incompatible change.

## Implementation

- `chars` is a text method in the shared dispatcher
  (`aura-runtime/src/text_methods.rs`), so the interpreter and native code
  run one implementation. `ord` and `chr` call `text_methods::ord` and
  `text_methods::chr` from both backends; native code passes `chr`'s code
  and receives `ord`'s result as a machine word.
- MIR lowers string `min`/`max` without a runtime helper: the collection
  form reuses ADR-0065's reduction loop with a `clone` on each replacement,
  and the argument form compares the arguments left to right and clones the
  winner.
