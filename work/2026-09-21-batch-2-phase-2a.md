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

## Evidence

- Recorded as the stages complete.

## Open items

- None yet.
