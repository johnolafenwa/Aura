# ADR-0061: Collection-element loans and slice views

- Status: Accepted; element and entry loans, `lookup`, and the resource
  capability policy ratified in detail on 2026-09-21 (Batch 2–3 design
  checkpoint, sections A, C, H, and I); slice views and returned collection
  views ratified in the same checkpoint (section B) and folded in at phase
  2b
- Date: 2026-09-06; amended 2026-09-21
- Implementation: Phase 2a in progress
- Roadmap: Batch 2
- Extends: ADR-0016, ADR-0038, ADR-0040, and ADR-0044
- Related: ADR-0052, ADR-0054, and ADR-0058

## Authority and current boundary

The user approved natural collection access in the
[roadmap](../14-priority-roadmap.md). ADR-0038 implements roots, fixed fields,
tuple positions, and contained loans. Indexes, keys, and slice views require
this extension. Existing ordinary list/string slices produce owned copies.

## Accepted decisions

Context may provide scoped shared access to a collection element. In the
future surface, both `print(tags[0])` and `tags[0].clone()` must compose without
first demanding an implicit owned read. The second expression explicitly
produces a cloned owner when the element type supports cloning.

An independent owned value still requires a valid Copy operation, explicit
clone, removal, or other owner operation. Contextual shared access must not
turn assignment into an implicit clone or let a view masquerade as ownership.

Extend place/lifetime reasoning to list elements, dictionary entries, and
slice views, including exclusive mutable access and write-through. Reject
structural operations that could invalidate a live view. Conflicting access
must be diagnosed statically, with the origin and invalidating operation.
The view's lifetime cannot exceed its owner or cross an unauthorized task
boundary.

Slice views and owned slices are separate operations. Their exact source
selection is still to be designed; this approval does not silently change an
existing owned slice into an alias. Zero-copy access must compose with future
Array views and callable/iterator lifetimes under one ownership model.

## Remaining detailed design

### Open conflicts

**Joint Batch 1–2, ADR-0038 / ADR-0058:** design the lifetime-bearing callable
type jointly with collection loans. Batch 1 callable storage first covers
owned/by-value captures and bound methods on owned or Copy receivers;
ADR-0038 loan-capturing closure storage waits for this shared design. Neither
ADR may treat the other as independently responsible for the lifetime model.

### Detailed contract

Specify indexed/keyed place identity, index/key evaluation counts, bounds and
missing-key behavior, negative positions, and which operations invalidate each
view. Define disjoint element/range reasoning, replacement/removal, dictionary
rehashing, slice ranges/steps, and any conservative restrictions needed when
indexes are known only at runtime. Do not assume reallocation is the only
source of invalidation: shifting or replacing an element also matters.

Define returned collection views, borrowing iterators, lifetime-bearing
callable storage, and backend/interface representation before exposing them.
The language must retain deterministic errors and cleanup under optimization.

## Completion evidence required

Pin shared indexed calls, explicit indexed cloning, owned-read rejection,
mutable write-through, simultaneous disjoint/conflicting access, bounds and
missing keys, invalidation by insertion/removal/replacement, returned-view
lifetimes, and non-cloneable elements. Check evaluation order and both backend
results/diagnostics, including early-exit cleanup. Update AU3005/AU3006 guidance
to describe the new contextual access rules when implementation lands.

## 2026-09-21 — Batch 2–3 checkpoint ratification (phase 2a detailed design)

The owner delegated the checkpoint's decisions ("Pythonic without breaking
the fundamental needs and principles of Aura as a safe and highly
performant systems language"); every question was ratified as its
recommended option in
[17-batch-2-3-design-checkpoint.md](../17-batch-2-3-design-checkpoint.md).
This amendment folds the phase 2a decisions into this ADR; the section
numbers below are the checkpoint's.

### Element and entry places (A1)

The existing `view` statement and contextual access extend to index and key
expressions, with no new introducer. An **element place** is the projection
`[index]` of a list place; an **entry place** is the projection `[key]` of a
dictionary place. Both join `PlaceProjection` beside fields and tuple
positions; ancestor/descendant overlap applies unchanged, a projection
through an element to a fixed field is admitted, and so is an element of a
field. The access forms are: `view name = items[i]` / `table[key]` (shared
element or entry loan on a view binding), `view mut name = ...` (exclusive
write-through loan; the collection place must be mutable), a bare
`items[i]` in a shared-read context (a contextual shared element loan for
the containing expression, which is how `print(tags[0])` and
`tags[0].clone()` compose), `items[i]` as a `mut self` receiver or `mut`
argument (a call-duration exclusive element loan), and `items[i]` in an
owned position (the existing owned read, `AU3005` for non-Copy elements
with `view` as the first suggestion). Compound assignment through a
non-Copy element stays `AU3006`; the guidance recommends a mutable entry
view. `set[T]` and `str` gain no element loans; `Array[T]` element views wait
for Batch 10.

### Selection, bounds, and absence (A2)

The index or key expression is evaluated exactly once when the loan
begins; a list index uses the `int64` index domain with one negative
normalization; an out-of-range position or an absent key traps with
`AU4003` at the loan-creating expression, exactly as a direct read does. A
live loan never re-evaluates its selector. A non-Copy key is retained as a
shared read for the probe and not consumed; the loan does not keep the key
alive. Bounds and presence are checked once at creation on both backends
before any descriptor is registered; the MIR validator proves that the
selector operand is evaluated before the loan begins and is not re-read.
The query forms with an explicit absent arm are the arm-scoped `lookup`
and the owned `Lookup[T]` results.

### Invalidation (A3)

Every collection operation is classified as an element-level write or a
whole-collection structural mutation, and the existing overlap rules apply
to the element projection. Disjointness is proven only between two element
projections whose selectors are both integer literals with different
values, or two entry projections whose selectors are literals of the same
key type with different values; every other pair overlaps conservatively.
List indexed assignment is an element-level write; dictionary indexed
assignment is structural. A structural mutation of a root, or a write to a
non-disjoint element, while an element or entry loan of that root is live
is refused statically (`AU3011`, naming the origin and the invalidating
operation). Invalidation is static: the validator refuses any MIR that
mutates a collection root or a non-disjoint element while an element loan
of that root is live, so a backend never sees a dangling element
descriptor; the runtime keeps only the checks it performs for root loans.

### Ownership through element views (A4)

No new ownership operation. Element views obey ADR-0038's aliasing rules
unchanged: a view never yields ownership of a non-Copy element (`AU3002`),
`view_binding.clone()` and `tags[0].clone()` produce owned values when the
element type is clone-safe, and moving an element out uses `pop`, `set`
(an element-level write whose transferred result is owned, still refused
under a live loan of the same or an unproven element), or `remove`.

### Iteration as element loans (A5)

`for x in items` holds one shared loan of the collection root for the loop
region and binds `x` on each iteration as a shared element loan; `for x in
mut items` holds one exclusive root loan and binds `x` as a mutable element
loan with immediate write-through; `for x in own items` is unchanged. The
loop binding is a first-class `ViewBinding` whose region ends on every
iteration edge; an element view may be reborrowed from it; it cannot be
moved out or stored; the body may take shared element loans of other
positions under bare iteration and may not under mutable iteration;
iteration through an existing view is a reborrow chain. Mutable iteration
writes through immediately rather than on the iteration edge, so a later
trap cannot discard an earlier successful write; `MutableIterationWriteback`
is retired from list lowering. Set iteration keeps its shared-only rule;
Queue and Range iteration yield owned values and are unchanged.

### Returned element and entry views (A6)

An element or entry projection may be the returned expression of a
`-> view [mut] T from origin` declaration, with the whole origin root as the
static footprint; the runtime token records the exact selected element, as
the returned-loan projection selector does for field alternatives today.
Exported callable metadata gains `Element`/`Entry` footprint kinds beside
whole-root and fixed-projection kinds (checked interface hash change).
Absence traps at the `return view` expression inside the callee before any
handoff. The stored-callable contract extends without change; a bound
method returning a view of `self` still cannot be packed; task targets
still cannot return views.

### Arm-scoped `lookup` and no app-facing `V | None` `dict.get` (C1, C2)

`lookup` is one arm-scoped operation on lists and dictionaries whose result
exists only as a `match` scrutinee and whose `Found` payload is an element
or entry loan (shared under a bare scrutinee, mutable under `match mut`).
There is no app-facing `V | None` `dict.get`: it would hide a clone of the
value.

### Resource capability policy and owned bindings (H1, H2)

The builtin receiver table is enforced at every call site through
`require_mutable_receiver`, after an audit that makes operations on Copy
runtime handles (`Queue[T]` puts and close, repeatable `Task[T]`
operations) shared receivers while every operation that mutates the value
or a non-Copy host resource keeps its documented `mut` requirement; the
audited table is the single source for the checker, the validator, hover,
and the Manual's API tables, with a characterization test. Every binding a
consuming form introduces (`match own` payload or type-pattern bindings,
`for ... in own` targets, tuple-unpack leaves of those forms) is a mutable
place, like the managed binding `with` creates.

### Script-scope narrowing (I)

Narrowing facts apply to entry-script bindings in phase 2a; value-position
`and` narrowing waits for Batch 4.

### Completion evidence for phase 2a

Failing fixtures land before their run-pass families; every run-pass and
run-fail fixture runs on MIR and forced direct execution with identical
stdout; every new MIR contract (element and entry loan selectors, the
lookup probe and its guarded loan, the removed iteration writeback) has a
forged-MIR test that fails at `run_mir` and `emit_host_native_object` with
one shared reason; the coverage floors move only upward; one complete local
chain and one green hosted run close the phase; the reference agent's diff
and pinned output are the usability evidence.
