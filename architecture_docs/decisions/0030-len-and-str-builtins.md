# ADR-0030: `len` and `str` builtins

- Status: Accepted
- Date: 2026-07-25
- Roadmap decision: Batch 2, Phase 3.5 Python expression kernel

## Context

Batch 2 requires "`len(x)` delegating to `.len()`" and "`str(x)` producing the
print/f-string rendering", and requires retiring the `python_len` and
`python_str` migration hints to acceptance. The roadmap fixes both meanings by
delegation and leaves the accepted domains, the name status, and the lowering
strategy to the provisional-decision protocol.

## Decision

- Both are maintained builtin functions, in the same namespace and with the
  same call-binding rules as `print`, `abs`, and `parse_int64`. They are not
  syntax and not loop forms.
- `len(value)` delegates to the value's own `len()` member and produces
  `int64`. The accepted domain is exactly the set of types that provide a
  builtin `len()` member, which in Aurora 0.1 is `String`, `Vec[T]`,
  `Map[K, V]`, and `Set[T]`. The domain is defined by that member rather than
  by an enumerated list, so a future type that gains `len()` is accepted
  without another decision. A value without the member reports `AU2002` and
  names the member the call would have delegated to.
- `String.len()`, `String.byte_len()`, `Vec[T].len()`, `Map[K, V].len()`,
  and `Set[T].len()` all produce `int64`. `String.len()` counts Unicode scalar
  values, while `String.byte_len()` counts UTF-8 bytes.
- `len(value)` and `value.len()` are the same operation with the same static
  result type, value, and ownership behavior: both produce `int64`, and
  `len()` borrows its receiver, so neither spelling moves anything.
- `str(value)` produces the same `String` that `print(value)` writes and that
  `f"{value}"` interpolates. It is total over the renderable surface rather
  than restricted to scalars, because the renderer is already total there;
  restricting it would make `str` weaker than the f-string it replaces.
- Both lower by delegation rather than to new runtime entry points: `len` to
  the receiver's `len()` member call, and `str` to a one-part format string.
  Both backends therefore reuse machinery they already have, and parity follows
  without direct-backend changes.
- Adding both names reserves them. A program may no longer declare
  `def len(...)` or `def str(...)`, the same way it may not redefine `print` or
  `abs`. This is deliberate and differs from the `enumerate`/`zip` loop forms in
  ADR-0029, which a user declaration does shadow: those two are loop forms with
  no value meaning, so a user function of the same name is unambiguous, whereas
  `len` and `str` are ordinary callables competing for the same namespace.

## Consequences

The two most common Python spellings work, and `str` in particular removes the
awkward `f"{value}"` workaround the retired hint used to recommend.

Reserving both names is a source-compatibility change: a program that declared
its own `len` or `str` is now rejected. This is recorded in the status page.
The alternative — shadowing, as `enumerate` and `zip` allow — was rejected
because it would make the meaning of a bare `len(x)` depend on whether an
unrelated declaration exists elsewhere in the module.

Defining `len`'s domain by the `len()` member rather than an enumerated list
means the diagnostic names the delegation target, which is the fact a caller
needs, rather than a list that would drift.

The B3.0-d amendment changes the five maintained public length members from
`int32` to `int64`. This is a source-compatibility change for annotations and
for code that passes a computed length to a still-`int32` boundary such as
`range(...)` or a Vec index. Such code must use an explicit checked
`as int32` cast; those index-domain APIs are not changed by this decision.

## Completion tests

- Focused compiler tests pin delegation over `String`, `Vec`, `Map`, and `Set`,
  the shared `int64` result of builtin and member length calls, the distinct
  Unicode-scalar and UTF-8-byte String counts, rendering equality between
  `str(x)` and `f"{x}"`, the two rejection categories, and the reservation of
  both names.
- A check-fail fixture pins the missing-`len()` rejection with its delegation
  help, and the run fixture pins exact stdout through MIR and the forced direct
  parity matrix.
- The `python_len` and `python_str` fixtures remain in the hint family and now
  assert that the Python spelling type-checks.
- The maintained example, the normative Expressions and API-index entries, the
  tutorial surface listings, the status page, and the conformance map are
  updated in the same freeze-rule pass.

## B3.0-d amendment and ratification

The Batch 2 checkpoint accepted the member-defined `len` domain, the total
`str` domain, and the name reservation, with one amendment: B3.0-d unifies all
five maintained public length members on `int64`. This restores the intended
identity that `len(value)` and `value.len()` have the same static type and
observable value. ADR-0030 is therefore **Accepted** with the amendment above.
