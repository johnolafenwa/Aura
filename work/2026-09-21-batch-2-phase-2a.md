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
  projection, span }`) and two runtime helpers,
  `aura_direct_element_path_load` and `aura_direct_element_path_store`,
  that walk one selection chain (`[i]`, `[k]`, and projection steps with a
  buffer of selector words) inside the collection in one call and read or
  replace only the value reached, so a nested view through an element or
  entry view (`view mut row = rows[0]; view mut cell = row[1]`) writes in
  place too; an owned dictionary key is held by the loan and released at
  `EndLoan`.

Defects met on the way, all fixed and pinned: `view name = users[1].name`
lowered to the tuple spelling `users.1.name` and trapped on both backends
(element projections); a field read through an element view failed on the
direct backend (`users.name`); an entry view with a `bool` or `int64` key
crashed the direct binary (a scalar key was passed unboxed); a string key
leaked once per read (the runtime borrows a lookup key); the checker
accepted `users[0].visits = 5` under a live view of `users[0]` and the
validator refused it as an internal error (the tuple/element bridge in
`PlaceProjection` and the indexed `member_access_path`); a nested element
view through an element view was refused by the validator (an element
reborrow selects inside its parent's value, not its parent's source) and
would have written into a clone on the direct backend (the prefix element
was cloned before the write), which the one-call selection chain removes.

## Evidence

- Fixtures (identical on MIR and forced direct execution): run-pass
  `element_view_shared`, `element_view_mutable` (write-through, class and
  string elements, literal-disjoint mutable views, evaluate-once `[7, 120,
  230]`), `entry_view_shared`, `entry_view_mutable` (`str`, `int64`, `bool`
  keys, class and union values), `element_place_assignment` (field
  assignment and compound assignment through elements and entries, a
  loop-binding reborrow), `element_view_nested` (views through element and
  entry views, written in place); run-fail `element_view_out_of_bounds` and
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
- Complete local chain: passed for the source and tests committed as
  `6df54d4b`; full evidence follows.

### Step 1 closeout (2026-09-22)

- Owner's delegated ruling, 2026-09-22: list `set(i, value)` is an
  element-level write with the same literal-disjointness requirement as
  indexed assignment, returning the old element owned; dictionary `set`
  remains whole-collection. The checkpoint A3 table and Q3 amendment and
  ADR-0061 A3 now agree with A4. The remaining structural operations keep
  their classifications.
- Resumed at `4abb93a0`. The existing instrumented report confirms the
  previous chain's compiler coverage failure: 96.3941% lines, 97.2761%
  functions, and 95.2449% regions. All three floors remain unchanged.
- Added checker projection/context tests, interpreter selector and place-walk
  tests, direct path tests, and lowering tests for projected writes through
  a collection view. The focused instrumented `adr0061_` run passed 15
  tests; incremental coverage reached 96.4668% lines / 97.4087% functions /
  95.2981% regions. This is development evidence, not a complete local chain.
- Public-boundary assertions now compare the complete shared validator
  reason across in-memory MIR, serialized MIR, and direct object emission
  (accounting for the interpreter's diagnostic wrapper).
  The first resumed full chain passed 2,130 compiler unit tests and exposed
  one older resource-budget test whose serialized-input guard intentionally
  precedes the flow validator. That test retains its original budget-category
  assertion at all three boundaries; new element-loan contracts keep exact
  shared-reason assertions. The corrected budget test passes in isolation.
- Direct execution of the new returned-collection selection test exposed a
  write-through defect: MIR updated the selected collection, but direct
  execution left its elements unchanged. The direct backend now passes the
  first opaque owner and its remaining static projection to the existing
  element-path walker instead of extracting a cloned collection first.
  Extending the test to union payloads exposed a second defect: element
  reborrows dropped a collection field projection. Lowering now preserves
  that projection in an ordinary parent reborrow before selecting an element.
  Neither fix changes the MIR contract or runtime ABI.
- The new `element_view_selected_collection` fixture pins both returned
  branch alternatives, nested list/entry selections, fields, tuple-held
  collections, and a union payload. Its complete pinned stdout matches MIR
  and forced direct execution. The focused instrumented suite passes 17
  tests after both fixes; clippy passes. The complete local chain below
  supersedes the incremental coverage numbers, which predate the fixes.
- The requested Daybreak Blue delegation could not start: the service
  returned `The Daybreak Blue model requires access_programs.cyber=daybreak_blue`.
  The primary agent continued the authorized defensive tests; no Daybreak
  review is claimed.

### Complete local closeout evidence (2026-09-22)

- `RUST_MIN_STACK=33554432 npm run ci` completed with exit 0 for the
  source and tests committed as `6df54d4b`. Tracked files remained unchanged
  throughout the chain (content digest
  `e6a57361f2a81d364196ab8c3cc5b482fc82f43b46965cd318d32729972cf5d3`).
- Compiler coverage: **96.4791% lines** (119391/123748), **97.4090%
  functions** (8083/8298), **95.3016% regions** (177564/186318).
  Floors remain 96.46 / 97.33 / 95.23.
- 2131 compiler unit tests and 378 CLI tests passed in the normal and
  instrumented runs. Backend parity passed all 518 run-pass and 97 run-fail
  fixtures. LSP tests passed 116/116; extension tests passed 28/28. LSP
  coverage remains 100% for statements, branches, functions, and lines.
- The complete chain also passed formatting, benchmark and baseline gates,
  packaging, identity, reference and tutorial checks, documentation build,
  dependency audits, clippy, and hygiene. The full local log is
  `/tmp/aura-b2-step1-ci.log`; the coverage export is
  `/tmp/aura-b2-step1-final-coverage.json` (local, disposable evidence copies).
- After that source gate, prose-only tutorial and Learn corrections describe
  explicit element/entry views and the remaining contextual-read limits.
  Fenced programs are unchanged. Generated LLM documents were refreshed;
  reference checks, tutorial checks, documentation build, and hygiene passed
  again (`/tmp/aura-b2-step1-doc-checks.log`).
- Hosted integration: [PR #17](https://github.com/johnolafenwa/Aura/pull/17),
  tested head `1e4891fb24d96485dcd329193fbe24f7145f3bea` (including the clean
  merge of `origin/main` at `6038fb28`). [CI run
  `35707901248`](https://github.com/johnolafenwa/Aura/actions/runs/35707901248)
  passed Ubuntu 24.04 and macOS 15 on its first attempt; no reruns were needed.
  Docs `35707901153` and Tutorial Examples `35707901218` also passed.
- Hosted compiler coverage, with the same unchanged floors:

  | Platform / job | Lines | Functions | Regions |
  | --- | --- | --- | --- |
  | Ubuntu 24.04 / `106681171662` | 96.4791% (119391/123748) | 97.4090% (8083/8298) | 95.3000% (177561/186318) |
  | macOS 15 / `106681172161` | 96.4767% (119388/123748) | 97.4090% (8083/8298) | 95.3005% (177562/186318) |

  Both hosted jobs retain 100% LSP coverage. Full job logs are saved locally
  as `/tmp/aura-b2-step1-ubuntu-ci.log` and
  `/tmp/aura-b2-step1-macos-ci.log`.
- Step 1 has met its local and hosted verification gates and is ready to
  merge. This evidence-only follow-up changes no compiler or test source.
  Phase 2a remains in progress; the open items below follow the step-1 merge.

## Item 1: contextual element reads (2026-09-23)

Codex's first attempt (saved as `f4c0eedd` on `codex/batch-2-phase-2a-2`)
introduced a new direct-backend model of borrowed runtime handles and a
`BeginUnionLoan` MIR instruction. Measured on 2026-09-23 it broke 76 of 539
existing run-pass fixtures on the direct backend, including returned views,
generic trait dispatch, and callables, and it broke check-pass, check-fail,
and run-pass fixtures on the interpreter as well. It was set aside and item
1 was rebuilt on `batch-2-phase-2a-item1` from `main`, with Codex's
contextual fixtures as the specification.

Design as landed:

- A contextual element use is a hidden element loan, exactly what an
  explicit `view` produces, so both backends already supported every case
  before the change: shared and `mut` arguments, receivers, `match` and
  `match mut` scrutinees, nested selections, and returned views of element
  arguments. The loan ends with its statement.
- Shared operand reads keep their existing read path. A read beside a live
  element loan goes through its own brief loan with a literal footprint, so
  `users[1]` stays readable while `users[0]` is mutably viewed.
- The checker raises `AU3005` only for a move of a non-copy element
  (binding, `own` argument, aggregate element, or a non-Copy field moved out
  of an element) and for a shared read of an element type that cannot be
  cloned. The guidance suggests a `view` first. Call-borrow and readability
  checks see element places with their selectors.
- Two bugs that also affect 0.3.4 were fixed on the way: a type-pattern
  `match` on a union-valued element view failed MIR validation, and a
  returned view through an element origin projected outside the element on
  the direct backend.

Left out: `element_contextual_union_arguments`. Borrowing a non-copy value
into a union parameter is refused for every place today (`AU2010`); that is
a separate feature, not a contextual read.

Evidence: every fixture passes on the interpreter and on forced direct
execution; `cargo test -p aura-compiler` passes; the complete local
`npm run ci` chain is green at `2b9fd0f0` (coverage 96.48 / 97.41 / 95.30
against floors 96.46 / 97.33 / 95.23).

## Item 2: invalidation classification, AU3011 (2026-09-23)

Every collection operation under a live element or entry view is now
classified as an element-level write or a structural mutation, and a
conflict reports `AU3011` naming the view, its origin, and the operation.
List indexed assignment, compound assignment, and `set` on a Copy element
are element-level writes, allowed when the selector is a literal different
from every live view. `set` on a non-Copy element stays whole-collection,
because returning the old element owned would need a move out of a live
slot. Dictionary assignment and every other mutating method are structural.
Merged as PR #22.

## H2 and I: owned bindings and script narrowing (2026-09-23)

Bindings introduced by consuming forms (`for ... in own`, their tuple
leaves, `match own` payloads and type-pattern bindings) are mutable places.
Narrowing applies to entry-script bindings, and a narrowed module constant
is re-tested locally with an `Unreachable` arm, which the direct backend now
compiles as a trap. H1 needed no user-visible change: Queue operations
already take shared receivers. The single-source audited receiver table
stays an internal follow-up. PR #23.

## C1: arm-scoped `lookup` (2026-09-23)

`match [mut] table.lookup(key):` on a named list or dictionary runs the
`Found` arm with an element or entry view and the `Missing` (or `_`) arm
otherwise. The checker and the lowering share one rewrite in
`sema/lookup.rs`: the key is evaluated once into a hidden local unless it is
a simple place, then an `if` on the presence test (`key in table`, or
`-len <= i < len`) runs the `Found` arm under `view [mut] name = table[key]`.
No MIR instruction was added, so both backends run it unchanged. A bare
`lookup` of a Copy element binds a copy, so the value can be returned. The
2026-09-23 ruling in ADR-0061 records the refusals: match expressions,
temporaries, `match own`, guards, and malformed arm sets.

Found on the way: the direct backend typed a field reached through an
element view as the element, so `team.members.append(x)` after `view mut
team = teams[0]` failed with "does not know dynamic method". Resolving the
view place now walks the projected fields
(`element_view_field_method_calls`).

Evidence: 26 `lookup_*` fixtures plus the regression fixture pass on both
backends; every fixture passes on both; all 731 documentation examples
compile.

## Open items

- Iteration unification (A5): retire `MutableIterationWriteback` for lists
  so `for x in mut items` writes through immediately.
- Returned element and entry views (A6): `return view items[i]` is still
  refused (`AU3004`), pinned by `element_view_return_unsupported`.
- Borrowing a non-copy value into a union parameter (`AU2010`), which
  `element_contextual_union_arguments` needs.
- The single-source audited receiver table (H1, internal).
- Phase 2a closeout: the final documentation sweep, the local chain, hosted
  CI, and the completion record in ADR-0061.
