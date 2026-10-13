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
- A defect found in the full test run: a `match` expression passed to a
  borrowed parameter moved a non-copy arm place, so a later use failed at
  run time with "unknown MIR place". Match arms now lend their places
  outside an owned position, as conditional expressions do
  (`match_expression_borrowed_argument`). The two union security tests that
  pinned the old rejection now check that the call runs on both backends.

Verification: the fixture suite passes; every new run-pass fixture prints
identical output on MIR and forced direct execution with a fresh native
cache. `cargo test -p aura-compiler -p aura` passes except
`package_io_helpers_cover_local_error_paths`, which fails only when the
tests run as root. Opened as PR #37.

The repository asks for security-sensitive regression tests to go to a
Daybreak Blue sub-agent. That model was not available in this session, so
the primary agent wrote the updated union security tests and the span
validator tests.

## Phase 2b

Goal: `Span[T]` slice views, returned range views, and the dictionary
lookup review from section M3.

Design as built. In the checker, `Span[T]` is a builtin view-only type. A
slice view's loan covers the whole base list. In MIR, a span is a
whole-list loan, or a list parameter, plus two hidden `int64` locals for
the absolute start and the length. A `Span[T]` parameter becomes a list
parameter plus hidden keyword-only start and length parameters. Every span
operation calls a private `__span_*` list member with absolute positions,
and both runtimes check each range against the list (`AU4003`). A function
returning `view Span[T] from p` returns its absolute `(start, len)` range,
and the caller lends the origin argument's base place with that range. The
validator treats the mutating span members as mutable borrows.

Work completed:

- `view part = values[a:b]`, nested spans, `Span[T]` and `mut Span[T]`
  parameters with slice, whole-list, and span arguments, indexing,
  iteration (`mut`, `enumerate`, `zip`, comprehensions), membership,
  equality with lists and spans, `len`, `index`, `count`, `reverse`,
  `sort`, `swap`, `fill`, `to_list`, and value slices of spans.
- Returned range views from functions, generic functions, methods with a
  parameter origin, trait methods through a generic bound, imported
  modules, and forwarding returns.
- Rejections: owned positions, fields, type arguments, union members,
  task capture, function values, lambda capture, rendering, structural
  members, an unbound span-returning call, and a method returning a span of
  its receiver.
- `dict` has no `get_optional`; an unknown `dict` member now reports
  `AU2001` with a pointer to `get` and `in`.
- Bug fixed on the way: `view mut w = items; w[2] = 9` failed MIR
  validation. Element assignment through a whole-list view now lowers as an
  element reborrow.
- Language analysis: span bindings hover as `Span[T]` with the range
  source, and member completion offers the span members.
- Fixtures: run-pass `span_view_*`, `returned_collection_view_ranges`,
  `returned_collection_view_imported`, and
  `dict_lookup_review_optional_values`; run-fail `span_bounds_*`,
  `span_index_out_of_range`, `span_argument_out_of_range`, and
  `returned_collection_view_range_out_of_range`; check-fail `span_view_*`,
  `returned_collection_view_*`, and `dict_lookup_review_get_optional`.
  Rust tests `span_validator` forge MIR for a mutating member through a
  shared loan, overlapping mutable ranges, and a range used after its loan
  ends.
- Documentation: Collections (`Span[T]` section), API Index, Types,
  Ownership and Borrowing, Diagnostics, Current Limits, tutorials 02 and 06,
  and `examples/collections/span_views.au` in the maintained example set.

Not built, and listed in Current Limits: spans of `str` or `Array`, a span
of a receiver field returned from a method, and span-bearing function
types.

## Verification

- PR #37 merged at `68b6080` after hosted verify passed on Ubuntu and
  macOS.
- PR #42 merged at `40ff76e` after hosted verify passed on Ubuntu and
  macOS at `541af66`. The first hosted run found two gaps, both fixed in
  the pull request. The docs build could not resolve an `@/` example
  embed, so the page uses a relative path and AGENTS.md shows that form.
  Compiler line coverage was 96.459% against the 96.46% floor, so checker
  and forged-MIR span tests were added.

## Follow-up

- A read of the scrutinee place inside a `match mut` arm also passes the
  checker and fails MIR validation with `AU4001`. It needs the read check
  scoped to the arm bodies; it is not part of this change.
- A runtime trap inside an imported module function names the entry file
  with the imported file's line and column. It predates this work.
