# ADR-0069: The builtin prelude: `Ord`, `Add`, `From`, and `Result` methods

- Status: Accepted
- Date: 2026-11-02
- Builds on: ADR-0055 and ADR-0058

## Context

Operators on a type parameter dispatch through traits named `Ord`, `Add`,
and so on, but Aura shipped no declaration of those traits and no
implementations for the builtin types. A generic `largest[T: Ord[T]]` needed
every program to declare `trait Ord` and then implement it for `int64` by
hand before `largest([3, 9, 4])` would check. `Result` had no methods, so
asking "did this succeed?" took a four-line `match`. The book called this out
as an honest cost. John approved closing both gaps on 2026-11-02 (rows 3 and
4 of the language-design questions).

## Decision

- A builtin module `prelude`, written in Aura (`crates/aura-compiler/std/prelude.au`),
  declares `Ord[Rhs]` (`lt`, `le`, `gt`, `ge`), `Add[Rhs, Out]` (`add`), and
  `From[Source]` (`from(value: own Source) -> Self`), with the same shapes the
  manual already documented.
- It implements `Ord[t]` and `Add[t, t]` for every integer type, `float32`,
  `float64`, and `str`, and `From` for each lossless widening: a narrower
  integer into a wider one of the same signedness, an unsigned integer into a
  wider signed one, and `int8`/`int16`/`int32`/`uint8`/`uint16`/`uint32`/`float32`
  into `float64`. Each body uses the builtin operator or `as`, which always
  takes precedence over trait dispatch, so nothing recurses.
- `Result[T, E]` gets `is_ok()`, `is_err()`, `unwrap_or(default)`, `map(f)`,
  and `map_err(f)` through `impl[T, E] ResultMethods[T, E] for Result[T, E]`.
  `unwrap_or`, `map`, and `map_err` take `own self`, as the payload moves out.
  There is no `unwrap`: a program that wants to stop on an error writes the
  `match`, so the failure path stays visible.
- Every module sees these names without an import. A module that declares or
  imports its own `Ord`, `Add`, or `From` keeps it, so existing programs that
  declare the traits themselves are unchanged.
- `Sub`, `Mul`, and the other operator traits stay undeclared for now; they
  can join the prelude later without breaking anything.

## Implementation

- The prelude is imported on demand. `check_with_context` checks a module as
  written; when the check fails with `unknown trait `Ord``, `Add`, or
  `From`, with an unsupported `is_ok`/`is_err`/`unwrap_or`/`map`/`map_err`
  call on a `Result`, or with a missing implementation of a `prelude.` trait
  bound required by another module, it adds `from prelude import <name>` and
  checks again (`sema/prelude.rs`). A program that names none of them, such as
  hello world, carries none of the prelude's items, so build time and binary
  size do not change for it.
- Specializing a generic function that returns a type parameter through
  `match` arms that each `return` left the unreachable fall-through
  `return None` in place, which the native backend rejected as a `None` to
  `int64` coercion. Specialization now replaces it with the specialized return
  type's default operand.

## Consequences

- `largest[T: Ord[T]]` works on `int64`, `str`, and user types that implement
  `Ord`, with no extra declarations.
- An error message about a missing prelude trait now names it `prelude.Ord`.
- A user trait named `Ord` with a different shape shadows the prelude one in
  that module, and the operators there use the user's trait.
