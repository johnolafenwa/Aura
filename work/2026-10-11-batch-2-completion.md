# Batch 2 completion: union borrowing and phase 2b

Started 2026-10-11 on `claude/batch-2-1sf9sm` from `main` at `10eee03`.
Scope: the `AU2010` union borrowing item that phase 2a left open, then
phase 2b of the
[Batch 2–3 design checkpoint](../architecture_docs/17-batch-2-3-design-checkpoint.md)
(section M3). Batch 3 and Batch 4 run in parallel threads. No version bump
or release is part of this work.

## Union borrowing (`AU2010`)

Goal: a bare `T | None` or other union parameter accepts a non-copy member
place without a copy, as a bare member parameter does.

Design. A bare parameter is shared access, so the callee can only read the
union. The argument lowers to the same element, entry, view, or
returned-view loan a bare member parameter would take, and the union is
injected from that loan for the call. No MIR contract changes: the
injection is the existing `UnionInject` of a borrowed operand, which both
backends already admit. A `mut` union parameter still requires the exact
union type (`AU2010`), because the callee may store a different member. An
`own` union parameter moves its argument, unchanged.

Work completed:

- The checker no longer rejects a borrowed non-copy member place at a bare
  union parameter (`validate_borrowed_union_injection` is removed).
- MIR lowering injects shared union arguments from a contextual element,
  entry, view, or returned-view loan (`lower_borrowed_union_injection`), so
  `describe(tags[1])` and `describe(table["k"])` pass validation.
- A defect found while testing: boxing a borrowed inline `str | None` for
  a generic `T | None` parameter on the direct backend passed a retained
  handle to `aura_direct_union_inject`, which consumes the value behind the
  handle and left the caller's string empty (`print(name)` printed `None`).
  A borrowed handle payload is now copied into the box, as the interpreter
  copies a place read into an injection. This also affected owned
  `str | None` locals passed to generic union parameters before this work.
- A checker gap found while testing: reading the collection inside its own
  `for ... in mut` loop (`len(words)`, `words[0]`) passed the checker and
  failed MIR validation with `AU4001`. The checker now reports `AU3002`
  with the loop's span.
- Fixtures: run-pass `union_borrow_member_places`,
  `union_borrow_element_places`, `union_borrow_generic_and_callables`,
  `union_borrow_resource_members`, and the former check-fail
  `union_injection_borrowed_noncopy` and `union_injection_borrowed_view`;
  check-fail `union_borrow_overlaps_mutable_argument` and three
  `mutable_iteration_rejects_*` fixtures.
- Documentation: Types (bare union parameters), Current Limits, tutorial
  10, and the `examples/enums/optional_arguments.au` example in the
  maintained example subset.

Verification: the fixture suite passes; every new run-pass fixture prints
identical output on MIR and forced direct execution with a fresh native
cache.

## Phase 2b

Not started.

## Follow-up

- A read of the scrutinee place inside a `match mut` arm also passes the
  checker and fails MIR validation with `AU4001`. It needs the read check
  scoped to the arm bodies; it is not part of this change.
