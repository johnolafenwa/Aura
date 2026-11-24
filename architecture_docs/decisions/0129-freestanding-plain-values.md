# ADR-0129: A freestanding build keeps tuples and enums of words in machine words

- Status: Accepted
- Date: 2026-11-24
- Builds on: ADR-0094 (freestanding builds), ADR-0128 (allocation hooks)
- Design checkpoint: [A Heap For Freestanding Builds](../40-freestanding-heap-design-checkpoint.md)

## Context

A hosted program keeps every tuple and every enum value in a box of the
runtime. A freestanding program has no runtime, so it refused both. The
owner chose route D1 of checkpoint 40 for the values of the language in
freestanding builds: a lean value layer of its own, with a rewrite of MIR,
and no change to the hosted runtime or to the emitter's hosted paths.

Many tuples and enums need no heap at all. When every part is a number, a
`bool`, a class of those, or another such tuple or enum, the value has a
fixed size and holds no reference.

## Decision

- **A pass over MIR rewrites those types to classes.**
  `native_codegen_freestanding_values.rs::lower_plain_values` runs in
  `prepare`, after unreached functions are dropped and before the subset
  check. The emitter already keeps a class of words in machine words, so
  it needs no new case.
- **A tuple becomes a class with one field for each element.** Building
  the tuple builds the class, and reading an element reads a field.
- **An enum becomes a class with a `tag` field and one field for each
  payload of each variant.** The tag is the variant's index. Building a
  variant writes its payloads and zeros in the other variants' fields.
  Reading a payload reads a field. A `match` becomes a comparison of the
  tag for each arm, and a wildcard arm becomes a jump.
- **The fields of the variants do not share space.** An enum value is as
  large as all its payloads together. Sharing is a later change to one
  function.
- **A class the pass makes needs drop exactly when the type it replaces
  did.** The ownership pass has already stated each reference by the
  original types. A class of words drops nothing either way, so the
  references still add up and the ownership checker accepts the module.
- **A type the pass cannot rewrite is left as it is.** The subset check
  then reports it as `AU2052`, as before. That covers an enum that holds
  text, a collection, a union, or an `indirect` payload.
- **`try` on such a `Result` becomes a test of the tag.** On `Err` the
  function returns the error in its own result type, after the `From`
  conversion the checker chose when the error types differ. Both results
  must need no drop. Any other `try` is left and reported.

## Consequences

- A freestanding program uses tuples, enums, `Result`, `match`, and `try` on
  them with no heap and no hook.
- The pass is the place where the next values of the language arrive:
  values that need a heap block become calls of functions written in Aura
  over the allocation hooks.
- A hosted build is unchanged.
