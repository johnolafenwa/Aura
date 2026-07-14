# ADR-0014: Map literals and indexing

- Status: Accepted
- Date: 2026-07-14
- Reference gap: duplicate literal keys and indexed read/assignment ownership

## Context

Aurora's `Map[K, V]` surface promises unique keys and insertion order, but the
MIR runtime previously retained duplicate equal keys created by one literal.
That made `len()`, lookup, and later replacement disagree with the ordinary map
model and with the direct backend. The existing indexed-read surface also
cloned non-copy values implicitly and classified a missing-key runtime trap as
the compile-time catch-all `AU2999`.

The completion audit then exposed a second ownership distinction: simple
indexed assignment can store an owned value of any type, while compound
assignment must first read the existing stored value. Leaving those two forms
under one undifferentiated rule would either reject valid non-copy replacement
or permit a hidden clone/destructive read during compound assignment.

## Decision

- Map-literal entries are evaluated from left to right, key before value.
- When a later literal entry has a key equal to an earlier entry, its value
  replaces the earlier value while the key retains its first insertion slot.
- `map[key]` returns the stored value only when `V` is a copy type. For a
  non-copy `V`, checking rejects the indexed read and directs callers to
  `get(key)` for the existing explicit cloned optional read or `remove(key)`
  to transfer ownership. This rejection uses `AU3005`.
- A missing key in an indexed read traps at runtime with `AU4003`.
- Simple indexed assignment, `map[key] = value`, inserts an absent key or
  replaces an equal existing key; it does not trap merely because the key was
  absent. It accepts any `V`. Both storage positions are owned: a non-copy key
  is consumed for insertion or replacement, and a non-copy right-hand value is
  consumed into the map.
- Simple indexed assignment evaluates and captures the key before it evaluates
  the right-hand value. Side effects in the value expression therefore cannot
  change which already-evaluated key is stored.
- Compound indexed assignment, `map[key] op= rhs`, is permitted only when `V`
  is copyable. It copies the current stored value, evaluates `rhs`, applies the
  operator, and stores the result. A missing key traps with `AU4003` during the
  initial read. For non-copy `V`, an implicit clone would hide ownership and
  moving/removing the stored value before the operation would make failure
  destructive. Such code is rejected with guidance to use `get(key)` for an
  explicit cloned optional read or `remove(key)` for an explicit ownership
  transfer followed by a simple assignment. This rejection uses `AU3006`.

This is a contained gap-fill guided by P1 (no plausible-but-wrong map model),
P2 (backend parity), P3 (Python-compatible duplicate-key value behavior), P4
(no hidden clones), and P6 (the smallest surviving surface). It adds no new
syntax or collection operation.

## Completion tests

- `crates/aurora-compiler/tests/fixtures/run-pass/map_literal_duplicate_keys.au`
  pins last-value/first-slot behavior plus key-before-value side effects for
  both literal construction and simple indexed assignment, and is exercised by
  the MIR/direct parity matrix.
- `crates/aurora-compiler/tests/fixtures/check-fail/map_index_non_copy_requires_explicit_clone.au`
  pins the non-copy ownership rejection.
- `crates/aurora-compiler/tests/fixtures/run-fail/map_index_missing_key.au`
  pins the `AU4003` trap on both backends.
- `crates/aurora-compiler/tests/fixtures/check-fail/map_index_assignment_consumes_noncopy_key.au`
  pins key consumption for indexed assignment.
- `crates/aurora-compiler/tests/fixtures/check-fail/map_compound_assignment_noncopy_value_rejected.au`
  pins the copy-only compound-assignment rule and its explicit `get`/`remove`
  exits.
