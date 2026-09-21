# Batch 2 phase 2a: element and entry loans

Started 2026-09-21 on `codex/batch-2-phase-2a` from `main` at `c308f9ba`
(the merge of the representation phase's third pull request). Design
authority: the ratified
[Batch 2–3 design checkpoint](../architecture_docs/17-batch-2-3-design-checkpoint.md)
(all 28 questions A under the owner's delegated instruction), folded into
[ADR-0061](../architecture_docs/decisions/0061-collection-element-loans-and-slice-views.md)'s
2026-09-21 section with dated amendments of ADR-0038, ADR-0044, ADR-0014,
ADR-0016, ADR-0005, and ADR-0006 (`0a51a85f`). Scope is the checkpoint's
phase 2a (section M2): element and entry loans, iteration unification, the
arm-scoped `lookup`, the resource capability policy, and script-scope
narrowing. Slice views and returned range views are phase 2b;
initialization and context managers are phases 3a and 3b.

## Target

- `view name = items[i]` / `table[key]` and `view mut name = ...` create
  shared and exclusive element and entry loans; a bare `items[i]` in a
  shared-read context is a contextual element loan, so `print(tags[0])` and
  `tags[0].clone()` compose; an owned read of a non-Copy element stays
  `AU3005` with `view` as the first suggestion; compound assignment through
  a non-Copy element stays `AU3006` with the mutable-view guidance.
- Selection is evaluated once at loan creation; an out-of-range position or
  an absent key traps with `AU4003` at the loan-creating expression on both
  backends; a live loan never re-evaluates its selector.
- Every collection operation is an element-level write or a structural
  mutation; disjointness is literal-only; a structural mutation or a
  non-disjoint element write under a live element or entry loan is
  `AU3011`, naming the origin and the operation; invalidation is static.
- List iteration is one root loan plus a first-class element `ViewBinding`
  per iteration (`mut` writes through immediately; `own` unchanged);
  `MutableIterationWriteback` is retired for lists.
- An element or entry projection may be a returned view with the whole
  origin root as the static footprint (`Element`/`Entry` footprint kinds in
  exported metadata; checked interface hash change).
- `list`/`dict` gain the arm-scoped `lookup` whose `Found` payload is a
  shared or mutable element/entry loan; no app-facing `V | None` `dict.get`.
- The audited builtin receiver table is enforced at every call site;
  consuming-form bindings are mutable places; narrowing facts apply to
  entry-script bindings.

## Execution order

1. Failing fixture families first (check-fail with `.diag` sidecars):
   `element_view_*`, `entry_view_*`, `element_contextual_read_*`,
   `element_invalidation_*`, `element_bounds_*`, `iteration_element_loan_*`,
   `lookup_arm_*`, `resource_capability_*`, `script_scope_narrowing_*`; then
   the run-pass and run-fail families with `.stdout` sidecars, identical on
   MIR and forced direct execution.
2. Checker: `PlaceProjection::Element(selector)` and `Entry(selector)` in
   `sema/places.rs`; `view_place` and `Stmt::View` checking; contextual
   element reads at arguments, receivers, operands, and scrutinees; the A3
   classification on every `BuiltinMember`; loop bindings as `ViewBinding`
   (retire `frozen_places` for lists); `lookup` as a compiler-known member
   with scrutinee-only checking; the audited receiver table in
   `require_mutable_receiver`; owned pattern and loop bindings as mutable
   places; narrowing facts for entry-script bindings.
3. MIR and validator: `BeginLoan`/`Reborrow` gain `selector: Option<Operand>`;
   the lookup probe becomes `Probe { target, collection, key }` plus a
   guarded `BeginLoan`; `MutableIterationWriteback` leaves list lowering;
   the validator proves evaluate-once selectors, presence-dominated lookup
   loans, and the A3 invalidation rule over calls and stores. Every new
   contract has a forged-MIR test refused at `run_mir` and
   `emit_host_native_object` with one shared reason.
4. Backends: the interpreter's loan arena descriptor gains an element or
   entry selector resolved against the collection's storage; the direct
   backend's `DirectViewPlace` alternatives gain a computed selector value.
   Neither backend clones an element and writes it back.
5. Manual chapters (Ownership and Borrowing, Collections, Statements,
   Execution Model, Enums and Match, Static Semantics, Diagnostics, Current
   Limits, API Index, Process, Filesystem, Network, Concurrency), examples,
   tutorials, generated documents, editor grammar; the reference agent's
   diff and pinned output as usability evidence; CHANGELOG; task board.
6. One complete local `npm run ci` chain and one green hosted run; floors
   move only upward; LSP coverage stays at 100%.

## Step 1: the element and entry loan vertical slice (sites surveyed 2026-09-21)

The first code stage lands `view [mut] name = items[i]` and
`view [mut] name = table[key]` end to end before contextual reads,
invalidation classification, iteration, `lookup`, the receiver audit, and
narrowing follow in their own commits.

1. Checker places (`sema/places.rs`): `PlaceProjection` gains
   `Element(ElementSelector)` and `Entry(EntrySelector)` where a selector is
   a literal (`int64` for lists; `str`/`int64`/`bool` literals for keys) or
   `Dynamic`; `ProjectionPath::overlaps` keeps prefix overlap and adds the
   literal-only disjointness rule (two literal selectors of the same kind
   with different values are disjoint; a `Dynamic` selector overlaps every
   selector of its kind); `Display` renders `[0]`, `["key"]`, and `[?]`;
   `place_path_type` projects a list's element type and a dictionary's
   value type. `view_place` (`sema/loans.rs:764`) accepts `Index` on a
   `list[T]` or `dict[K, V]` object as an element or entry projection
   (the `AU3004` "indexed collection elements do not have stable view
   identity" refusal ends; the tuple arm is unchanged); `Stmt::View`
   checking (`sema.rs:3569`) needs no new rule for the mutable-source and
   loan-availability checks because they run on the place path.
2. MIR contract (`mir.rs:741`, `752`): `BeginLoan` and `Reborrow` gain
   `#[serde(default)] selector: Option<Operand>`; the source string stays
   the collection place, so every existing consumer that keys on the
   place string is unchanged. Lowering (`mir.rs:10919`): for an index
   source, evaluate the index or key once into a typed temporary before
   the loan begins and pass it as the selector; the loan's local type is
   the element or value type.
3. Validator (`mir.rs:1259`, `7397`): a selector operand must be a place
   assigned before the loan in the same block or a literal; it is never
   re-read for the loan; an element loan's source must be a `list` or
   `dict` place; forged-MIR tests refuse a re-evaluated selector, a loan
   whose source is not a collection, and a selector of the wrong type at
   `run_mir` and `emit_host_native_object` with one reason.
4. Interpreter (`mir_runtime.rs:1029` `begin_loan`, `1006`
   `resolve_loan_place`): the loan record gains the resolved selector
   value; creation checks bounds or presence once (`AU4003` at the
   loan-creating expression) before registering; `ReadLoan` and
   `WriteLoan` resolve the element or entry in the collection's storage;
   a negative list index normalizes once as `len() + index`.
5. Direct backend (`native_codegen.rs:5642` `BeginLoan`,
   `resolve_view_place`, `DirectViewPlace`): a view place gains a computed
   selector value; element reads and write-through go through runtime
   helpers on the collection handle that borrow or replace the element in
   place (never clone-and-write-back); bounds and presence trap with
   `AU4003` at creation on this backend too; Copy elements may be copied
   bits as a shared view of a Copy place is today.
6. Fixtures: `element_view_shared`, `element_view_mutable`,
   `entry_view_shared`, `entry_view_mutable`, `element_view_field`
   (`users[i].name`), `element_view_of_field` (`self.tags[0]`),
   `element_bounds_*` (run-fail `AU4003` on both backends),
   `element_view_evaluate_once` (`[11, 20, 30]` after rebinding the
   index); the existing `view_index_place_rejected` check-fail fixture
   becomes a run-pass fixture.

## Risks and rules

- Coverage floors (96.46 / 97.33 / 95.23) may only rise; production code
  stays under the coverage regex; error paths use eager formatting rather
  than closures the tests never run.
- No unsupported case hides behind automatic backend fallback; every
  feature works on both backends.
- Heavy builds observe the `target/` and disk policy (prune
  `target/debug/deps` and the coverage target between chains).
- Fixture stems pass the identity gate; sidecars end without a trailing
  blank line.

## Step 1 as landed (2026-09-21)

The vertical slice landed with two departures from the survey above, both
narrower in mechanism and wider in what programs it accepts:

- MIR keeps `BeginLoan`/`Reborrow` untouched and adds `BeginElementLoan
  { loan, source, selector, projection, mutable, span }` and
  `ReborrowElement { loan, parent, selector, projection, mutable, span }`.
  `selector` is a literal (`Int`, `String`, `Bool`) or a declared undotted
  local copied before the loan; `projection` is the dotted path inside the
  element (`visits`, `0`, `inner.name`), empty for the element itself;
  `span` is the index expression, named by the `AU4003` trap on both
  backends. The validator's footprint of a literal-selected loan is the one
  slot (`users.[sel:i:0]`, `[sel:s:<hex>]`, `[sel:b:<bool>]`), so two
  literal-disjoint mutable element loans of one collection validate as the
  checker allows; a place selector keeps the whole collection.
- The checker's contextual reads landed for one context now rather than
  later: an index used as the object of a field access is a place read
  (`users[i].visits`, `people[key].name`), and an assignment whose target
  projects inside an element (`users[i].visits = 5`, `+= 1`) lowers to a
  statement-scoped mutable element loan (`BeginElementLoan`/`WriteLoan`/
  `EndLoan`), never a clone-and-write-back. `member_access_path` spells
  indexed places (`users[0].visits` is `users.0.visits`, an element under
  the overlap rule; a computed index is a dynamic selector), so the view
  lock rules (`AU3002`) apply to element writes. A list element or entry as
  a `mut` argument or mutating-method receiver stays refused (the argument
  message gains a `view mut` help) until contextual arguments land.
- The interpreter encodes the selected slot as a place segment (`[i:N]`,
  `[k:i:N]`, `[k:b:<bool>]`, `[k:s:<hex>]`) followed by the projection; the
  direct backend's `DirectViewAlternative` gains `elements`
  (`DirectElementSelector { variable, selector_ty, element_type, kind,
  projection, span }`) and four runtime helpers
  (`aura_direct_vec_index_path`, `aura_direct_vec_set_index_path_in_place`,
  `aura_direct_map_index_path`, `aura_direct_map_set_index_path_in_place`)
  that read or replace the projected value inside the element in place; an
  owned dictionary key is held by the loan and released at `EndLoan`.

Defects met on the way, all fixed and pinned: `view name = users[1].name`
lowered to the tuple spelling `users.1.name` and trapped on both backends
(element projections); a field read through an element view failed on the
direct backend (`users.name`); an entry view with a `bool` or `int64` key
crashed the direct binary (a scalar key was passed unboxed); a string key
leaked once per read (the runtime borrows a lookup key); the checker
accepted `users[0].visits = 5` under a live view of `users[0]` and the
validator refused it as an internal error (the tuple/element bridge in
`PlaceProjection` and the indexed `member_access_path`).

## Evidence

- Fixtures (identical on MIR and forced direct execution): run-pass
  `element_view_shared`, `element_view_mutable` (write-through, class and
  string elements, literal-disjoint mutable views, evaluate-once `[7, 120,
  230]`), `entry_view_shared`, `entry_view_mutable` (`str`, `int64`, `bool`
  keys, class and union values), `element_place_assignment` (field
  assignment and compound assignment through elements and entries, a
  loop-binding reborrow); run-fail `element_view_out_of_bounds` and
  `entry_view_missing_key` (`AU4003` at the index expression); check-fail
  `element_view_write_locked`, `element_view_dynamic_locks_collection`,
  `element_view_source_mutated` (`AU3002`), and the earlier
  `view_index_immutable_source`.
- Forged-MIR contracts refused at `run_mir`, `run_serialized_mir`, and
  `emit_host_native_object` with one reason: same-slot and dynamic overlap,
  malformed projection, undeclared and unsupported selectors, a loan whose
  source is an active loan, a mistyped loan local, and an element reborrow
  of an inactive parent (`adr0061_element_loan_contracts_are_refused_at_every_public_boundary`).
- Unit tests: the checker's contextual reads, refusals, and lock rules
  (`adr0061_element_places_read_fields_in_place_and_refuse_element_arguments`);
  the direct view-place projection inside an element and the direct
  compilation of every element and entry form
  (`adr0061_direct_view_place_projection_reaches_inside_an_element`,
  `adr0061_direct_codegen_compiles_element_and_entry_loans`); the runtime
  path helpers' reads, in-place writes, and traps
  (`adr0061_direct_element_path_helpers_read_and_write_inside_elements`).
- Chain: recorded below when the local `npm run ci` completes.

## Open items

- Contextual element reads at arguments, receivers, operands, and
  scrutinees (checkpoint M2) are the next step; until then a list element
  or entry cannot be a `mut` argument or a mutating-method receiver, and a
  Copy read of `users[1].visits` under a live mutable view of `users[0]` is
  refused conservatively (the read rule checks the collection root).
- `A3` invalidation classification (`AU3011` naming the origin and the
  operation) is not yet in: a structural mutation under a live element view
  is refused by the existing `AU3002` view lock.
- Nested element views (`grid[i][j]`, `table[key][i]`) are refused by the
  copy rule at the inner index until contextual reads land; the lowering
  and both backends already accept a chain of element selectors.
