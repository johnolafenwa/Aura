# Batch 2–3 design checkpoint: natural collection access, initialization, and resource management

Status: **Ratified 2026-09-20 by delegated decision; implementation follows the Batch 1 representation phase**

Date: 2026-09-20. Source baseline: `ae106162` (main after the H2 merge,
[PR #13](https://github.com/johnolafenwa/Aura/pull/13)); the H2 work record
and task board are dated 2026-09-20.

This is the joint design checkpoint for [roadmap Batch 2](14-priority-roadmap.md#priority-batches)
(natural collection access) and [Batch 3](14-priority-roadmap.md#priority-batches)
(initialization and resource management). It follows the structure and
ratification procedure of the
[Batch 1 checkpoint](16-batch-1-design-checkpoint.md): each lettered bundle
states a recommendation, gives the reasoning with Aura examples, and compares
the alternatives; the [questionnaire](#ratification-questionnaire) lists every
question with an empty **Ratified** line for the owner. The design below
describes the proposed target, not current compiler behavior. Where the text
describes current behavior it cites the source or fixture that pins it.

The ten [Approved Decisions](14-priority-roadmap.md#approved-decisions) remain
binding. Decisions 3, 4, and 5 ([roadmap lines 181–193](14-priority-roadmap.md))
are the constraints here: `__init__(self, ...)` defines the class call when
present, complete field initialization with no partially initialized escape,
named factories returning `Result[Class, Error]` for fallible creation;
generic managers, multiple resources, typed entry/exit, exit registered after
successful entry, body failure primary; scoped shared element access where
context permits, explicit owned reads, rejection of structural operations
that could invalidate a live element view. There is no vote to reintroduce
hidden cloning, garbage collection, a general destructor hook, or a
compatibility layer for any removed spelling.

The inputs are [ADR-0038](decisions/0038-place-based-loans-and-views.md)
(the implemented loan model, including its open conflicts at
[lines 20–32](decisions/0038-place-based-loans-and-views.md)),
[ADR-0061](decisions/0061-collection-element-loans-and-slice-views.md),
[ADR-0059](decisions/0059-custom-initialization.md),
[ADR-0060](decisions/0060-typed-context-managers.md),
[ADR-0052](decisions/0052-anonymous-closed-union-types.md) (a union member
may not be a view type, [lines 116–117](decisions/0052-anonymous-closed-union-types.md)
and [279–283](decisions/0052-anonymous-closed-union-types.md)),
[ADR-0058](decisions/0058-first-class-callables-and-binding-contracts.md)
(the joint lifetime-bearing callable/view design,
[lines 87–92](decisions/0058-first-class-callables-and-binding-contracts.md)),
[ADR-0062](decisions/0062-typed-serialization-validation-and-schemas.md)
(decode reuse of the cleanup mechanism, [lines 45–48](decisions/0062-typed-serialization-validation-and-schemas.md)),
[ADR-0063](decisions/0063-everyday-syntax-and-pattern-ergonomics.md) (class
patterns need their own access model, [lines 27–29](decisions/0063-everyday-syntax-and-pattern-ergonomics.md)),
and the earlier collection decisions [ADR-0014](decisions/0014-map-literals-and-indexing.md),
[ADR-0016](decisions/0016-retained-noncopy-expression-borrows.md),
[ADR-0040](decisions/0040-owned-vec-and-string-slices.md), and
[ADR-0044](decisions/0044-canonical-collection-surface.md). The Batch 1
checkpoint's own statements about Batch 2 are binding inputs: the Q23
follow-up ([lines 1078–1085](16-batch-1-design-checkpoint.md)), the C10 loan
storage deferral ([lines 1000–1005](16-batch-1-design-checkpoint.md)), the
reference-agent limitation ([lines 1435–1441](16-batch-1-design-checkpoint.md)),
and the unresolved cancellation conflict in its reconciliation table
([line 1673](16-batch-1-design-checkpoint.md)). The H2 work record's
[open items](../work/2026-09-19-batch-1-phase-2.md) (lines 184–195, 209–213,
and 221–225) are inputs that this checkpoint must dispose of.

The implemented baseline is described in the Manual's
[Ownership and Borrowing](../docs/manual/ownership-and-borrowing.md),
[Collections](../docs/manual/collections.md), [Classes](../docs/manual/classes.md),
[Statements](../docs/manual/statements.md), [Execution Model](../docs/manual/execution-model.md),
[Closures](../docs/manual/closures.md), [Diagnostics](../docs/manual/diagnostics.md),
and [Current Limits](../docs/manual/current-limits.md). The maintained
usability oracle is [tool_runner version 1](../examples/agents/tool_runner/src/main.au).

All code and grammar fences in this proposal are plain `text`. They are design
examples, not executable reference claims. No new reserved keyword is proposed.
`view`, `with`, `match`, `mut`, `own`, `def`, `as`, and `try` supply the
notation; `Span`, `Lookup`, and the dunder method names `__init__`,
`__enter__`, and `__exit__` are builtin type names or ordinary identifiers
with a declared shape, not lexical keywords.

## Ratification record

On 2026-09-20 the owner delegated the ratification of this checkpoint with
one instruction: make the decisions "aligning to being pythonic without
breaking the fundamental needs and principles of Aura as a safe and highly
performant systems language". Under that instruction every recommendation
is ratified as written: **all 28 questions: A**. The original alternatives
stay in the questionnaire with a separate **Ratified** line per question.

The instruction was applied question by question. Where the Python shape and
the safety or performance principle agreed, the recommendation already had
that shape: element access through the existing `view` and contextual
spellings (Q1), an `IndexError`/`KeyError`-like trap on an invalid selector
(Q2), `def __init__(self, ...)` with per-field definite initialization
(Q12, Q15), `__enter__`/`__exit__` managers with `with a, b:` (Q17, Q19),
and owned pattern and loop bindings that need no `mut` ceremony (Q26).
Where the Python shape pulled against a principle, the principle decided:

- **Q10:** Python's `dict.get(key)` returns the value or `None`. An owned
  `V | None` result would hide a clone for every non-Copy value and would
  flatten a stored `None`; both contradict the no-hidden-cloning rule and
  ADR-0052. The borrowing `lookup` outcome (Q9) gives the zero-copy access an
  application needs, and `dict.get -> Lookup[V]` stays the owned form.
- **Q16:** Python's `__init__` raises on failure. Aura has no exceptions, and
  a `Result`-returning `__init__` would turn every class call into a
  `Result` expression; named factories returning `Result[Class, Error]` are
  the approved decision and keep the class call total.
- **Q22:** exits run with cancellation pending and no grace period, the
  Trio rule Aura's structured concurrency already follows; a masked grace
  period would trade a bounded cleanup promise for unbounded latency.
- **Q25:** enforcing the builtin receiver table uniformly breaks programs
  that close a resource through a shared binding; that is the sound
  direction, and the clean-slate policy has no compatibility layer.
- **Q7:** a slice in value position stays an owned copy, as in Python; the
  borrowing form is the explicit `view` over a range with the view-only
  `Span[T]` pointee, so no copy is hidden and no second representation of
  `list[T]` is introduced.

The ratification record is complete. ADR-0059, ADR-0060, and ADR-0061 keep
their "accepted direction" status until Batch 2's first stage reconciles
their bodies with these answers, as Batch 1's first stage did for its ADRs.
Implementation begins after the Batch 1 representation phase merges, in the
order M1 gives: 2a, 2b, 3a, 3b, each with failing fixtures first, both
backends, validator pins at `run_mir` and `emit_host_native_object`, and one
green hosted run per merge.

## Scope

**In scope.** Batch 2: list element and dictionary entry loans (`view x =
items[i]`, `view mut x = table[key]`, contextual shared element reads), the
invalidation rules for structural mutation, the ownership rules for reading,
cloning, and moving elements, iteration as element loans, returned element
views, slice views over lists, and the app-facing dictionary lookup review
required by Q23. Batch 3: `__init__(self, ...)`, definite initialization,
named fallible factories, typed context managers (protocol, generic managers,
multiple header items, entry views from header temporaries, exit failure and
cancellation ordering), and the one partial-construction cleanup mechanism
extending ADR-0038's ordered exit-action stack. Cross-cutting: the resource
capability policy from the H2 open item, the uniform enforcement of builtin
receiver capabilities, and the disposition of the two H2 narrowing gaps.

**Out of scope.** Stored loan captures and captured-self returned views
(the joint lifetime-bearing storage design; section D states what Batch 2
fixes now and where the rest is owned), `Array[T]` element and strided views
(Batch 10), `str` slice views (deferred in B2), set element loans (sets have
no place identity and no index syntax), borrowing iterators and generators
(Batch 9), decorators (Batch 6), class patterns (Batch 4 under ADR-0063),
typed decoding itself (Batch 6; only the cleanup mechanism is shared), any
FFI or ABI claim, and any change to the ratified Batch 1 answers.

**Constraints honored throughout.**

- Clean-slate policy ([ADR-0057](decisions/0057-clean-slate-pre-adoption-policy.md)):
  where an existing spelling or rule changes, the old form has no
  compatibility layer, migration diagnostic, or quick fix.
- Every feature lands on both maintained backends through the thin boundary
  described in [15-backend-boundary.md](15-backend-boundary.md), with the
  forced MIR/direct parity matrix as the gate. Validator refusals are pinned
  with the same reason at `run_mir` and `emit_host_native_object`, as the
  Batch 1 security tests do ([batch1_validator_coverage.rs:6–57](../crates/aura-compiler/tests/batch1_validator_coverage.rs)).
- Coverage floors (96.46% lines / 97.33% functions / 95.23% regions after
  H2, [work record lines 156–158](../work/2026-09-19-batch-1-phase-2.md)) may
  only be raised.
- The representation phase (Q9 A / Q15 A / Q16 A: inline union layout and
  four-word callables) runs in parallel. Batch 2 element loans must not
  assume the interim boxed union payload or `Arc` closure environment that the
  Manual discloses ([closures.md:452–455](../docs/manual/closures.md)). They
  may assume exactly three things: (1) the loan contract is place identity —
  root, normalized projection, storage generation — and never an address;
  (2) between structural mutations, a list element or dictionary entry is
  stable addressable storage for the element's canonical representation on
  both backends, a contract the runtimes uphold rather than a layout the
  compiler observes; and (3) a union-typed element is reached through the
  same canonical payload projections and tag tests that fields already use
  (`UnionTagTest` and payload projections, [15-backend-boundary.md:252–262](15-backend-boundary.md)),
  whether the payload is boxed today or inline later. They may not assume an
  element stride, pointer identity across reallocation, an inline payload, or
  the ratified union layout. The `union_layout` plan's ABI note
  ([union_layout.rs:80–82](../crates/aura-compiler/src/union_layout.rs)) is
  metadata for the representation phase, not a Batch 2 dependency.
- The language stays Python-shaped with second-class borrows and no garbage
  collector: every loan is a binding or a call-duration access, never a
  first-class value, and nothing here introduces reference counting of
  mutable state, hidden cloning, or a destructor hook.

## A. Element and entry loans

### A1. Indexed and keyed places — Q1

ADR-0038 admits roots, fixed fields, tuple positions, arm-scoped enum
payloads, and reborrows as places and rejects `list` indexes and `dict` keys
"including constant indexes" ([ADR-0038:226–233](decisions/0038-place-based-loans-and-views.md)),
deferring them because indexed identity "additionally needs evaluate-once
index/key ownership, collection generation and reallocation rules, bounds
behavior, alias overlap, structural-mutation invalidation, and exact backend
parity" ([ADR-0038:235–240](decisions/0038-place-based-loans-and-views.md)).
The checker pins the rejection at
[loans.rs:782–789](../crates/aura-compiler/src/sema/loans.rs) and
[view_index_place_rejected.diag](../crates/aura-compiler/tests/fixtures/check-fail/view_index_place_rejected.diag)
(`AU3004: indexed collection elements do not have stable view identity`).
This section supplies each deferred item.

**Recommend extending the existing `view` statement and contextual access to
index and key expressions, with no new introducer**:

```text
def main():
    mut items = ["ada", "grace", "linus"]
    view first = items[0]
    print(first)
    print(items[1])
    copied = items[2].clone()

    mut counts: dict[str, int64] = {"ready": 1}
    view mut ready = counts["ready"]
    ready += 1
    print(counts["ready"])
```

The grammar already parses these forms: `view-statement` takes any
expression ([grammar.md:525–527](../docs/manual/grammar.md)) and
`index-suffix` is an ordinary postfix ([grammar.md:793–795](../docs/manual/grammar.md)).
Only static semantics and lowering change. An **element place** is the
projection `[index]` of a list place; an **entry place** is the projection
`[key]` of a dictionary place. Both extend `PlaceProjection` beside fields
and tuple positions; ancestor/descendant overlap continues to apply, so
`items` overlaps `items[i]` and `items[i]` overlaps `items[i].name`. A
projection through an element to a fixed field is admitted (`view name =
users[i].name`), as is an element of a field (`view tag = self.tags[0]`).

Three access forms follow from Approved Decision 5 and ADR-0061's accepted
decisions ([ADR-0061:19–26](decisions/0061-collection-element-loans-and-slice-views.md)):

| Source form | Access produced | Lifetime |
| --- | --- | --- |
| `view name = items[i]`, `view name = table[key]` | Shared element/entry loan bound to a view binding | Inferred last use, bounded by lexical scope, as for every ADR-0038 view |
| `view mut name = items[i]`, `view mut name = table[key]` | Exclusive write-through loan; requires a mutable collection place | Same |
| `items[i]` or `table[key]` in a shared-read context: a bare argument, a shared receiver, an operand of equality/rendering/comparison, an f-string interpolation, a bare `match` scrutinee, a bare iteration source | Call- or expression-duration shared element loan, exactly like a directly read returned view ([ADR-0038:186–189](decisions/0038-place-based-loans-and-views.md)) | Ends after the containing expression's final use |
| `items[i]` as a `mut self` receiver or `mut` argument | Call-duration exclusive element loan; requires a mutable collection place | Ends when the call returns |
| `items[i]` in an owned position: assignment to an owned binding, `own` argument, return, aggregate or collection storage, `match own` scrutinee | Copy element: ordinary copy. Non-Copy element: rejected with `AU3005` as today, with guidance updated to name `view`, `.clone()`, `pop`, `set`, and `remove` | — |

This makes `print(tags[0])` and `tags[0].clone()` compose without an implicit
owned read, which is the precise Approved Decision 5 obligation. The
contextual read is the same mechanism as reading a returned shared view
inside one containing expression; it introduces no new value category. A
Copy element read stays a copy on both backends because the loan is
unobservable, exactly as a shared view of a Copy place may be optimized to
copied bits ([ADR-0038:242–244](decisions/0038-place-based-loans-and-views.md)).

The alternative keeps index expressions out of the place grammar and adds
builtin returned-view methods only: `items.at(i) -> view T from self`,
`items.at_mut(i) -> view mut T from self`, `table.at(key)`. It reuses the
returned-view machinery unchanged and never needs an element projection kind
in the checker. It fails the usability goal: `print(tags[0])` would still be
`AU3005` for non-Copy elements, and `view first = items.at(0)` names the
same access twice. The recommended design keeps the method form as its
*internal* model — an element loan is checked and lowered like a returned
view whose origin is the collection and whose footprint is the selected
element — without exposing a second spelling.

Compound assignment through an element (`table[key] += rhs` for non-Copy
`V`) remains `AU3006` under ADR-0016's rule
([ADR-0016:46–56](decisions/0016-retained-noncopy-expression-borrows.md));
the guidance now recommends `view mut entry = table[key]` followed by
`entry = entry + rhs` or the type's own mutating method, which expresses the
read-modify-write as an explicit write-through instead of a hidden clone.
Copy-element compound assignment is unchanged.

`set[T]` gains no element loans: set elements have no index spelling, are
never mutated in place, and their identity is by equality. `str` has no
integer indexing ([collections.md:161–162](../docs/manual/collections.md)) and
therefore no element loans. `Array[T]` element views wait for Batch 10's
strided view model; the scalar element is Copy and its indexed compound
assignment already works.

### A2. Index and key evaluation, bounds, and absence — Q2

**Recommend evaluate-once selection at loan creation with the existing
direct-index failure contract**: the index or key expression is evaluated
exactly once when the loan begins; a list index uses the `int64` index domain
with one negative normalization as `len() + index`
([collections.md:121–123](../docs/manual/collections.md)); an out-of-range
position or an absent key traps with `AU4003` at the loan-creating
expression, exactly as a direct read does today
([dict_index_missing_key](../crates/aura-compiler/tests/fixtures/run-fail/dict_index_missing_key.au),
[list_index_out_of_bounds](../crates/aura-compiler/tests/fixtures/run-fail/list_index_out_of_bounds.au)).
A live loan never re-evaluates its selector; rebinding the index variable
afterwards does not retarget the view, mirroring ADR-0017's one-time source
selection for loops.

```text
def main():
    mut items = [10, 20, 30]
    mut position = 0
    view mut selected = items[position]
    position = 2
    selected += 1
    print(items)
```

This prints `[11, 20, 30]`: the loan selected position 0 once. The key of a
dictionary entry loan is used only for the hash and equality probe; a
non-Copy key expression such as a `str` local is retained as a shared read
for the probe and not consumed, as an indexed read retains it today
([ADR-0016:34–40](decisions/0016-retained-noncopy-expression-borrows.md)).
The loan does not keep the key alive.

The reason to trap rather than to return a lookup-shaped result is that the
`view` and index forms are *place selections*, not queries. A place that may
not exist is not a place. The query forms are the arm-scoped `lookup` in C1
and the owned `Lookup[T]` results of `list.get`/`dict.get`; each has an
explicit absent arm. The trap keeps `view e = table[key]` exactly as strict
as `value = table[key]` so that the two spellings never disagree about
absence.

| Absent-key policy for `view e = table[key]` | Consequence |
| --- | --- |
| Trap with `AU4003` (recommended) | Identical to direct indexing; presence is tested with `key in table` or handled through `lookup` |
| Lookup-shaped view-bearing result | Requires a view inside `Lookup[view V]`, which ADR-0038 defers ([ADR-0038:372–375](decisions/0038-place-based-loans-and-views.md)) and which is not a value type; C1 supplies the arm-scoped form instead |
| Insert-or-view (`defaultdict`-like) | Hidden insertion and a hidden default construction; rejected by the no-hidden-cost rule and by Approved Decision 5's explicit-ownership requirement |

Bounds and presence are checked once at creation on both backends before any
descriptor is registered; a failed creation registers nothing and drains
nothing. The MIR validator proves that the selector operand is evaluated
before the loan begins and is not re-read.

### A3. Structural mutation and invalidation — Q3

ADR-0061 requires that structural operations that could invalidate a live
element view be rejected statically "with the origin and invalidating
operation", and warns that reallocation is not the only source of
invalidation: "shifting or replacing an element also matters"
([ADR-0061:29–31, 52–57](decisions/0061-collection-element-loans-and-slice-views.md)).

**Recommend classifying every collection operation as either an element-level
write or a whole-collection structural mutation, and applying the existing
overlap rules to the element projection**:

| Operation on `items: list[T]` | Classification | Effect on live element loans of `items` |
| --- | --- | --- |
| `items[j]` shared read, `len`, `in`, `get`, `index`, `count`, `copy`, `map`, `filter`, slicing, bare iteration, bare `match`, passing `items` to a bare parameter | Shared access to `items` | Allowed while every live element loan is shared; rejected (`AU3002`) while a mutable element loan is live |
| `items[j] = value` (simple indexed assignment) | Element-level write to `items[j]`; replaces in place, never reallocates or shifts | Rejected (`AU3011`) unless `items[j]` is proven disjoint from every live element loan |
| `append`, `insert`, `extend`, `pop`, `remove`, `clear`, `reverse`, `sort`, `set`, `swap`, `reserve`, `for x in mut items`, passing `items` to a `mut` parameter or `mut self` method, `match mut items`, moving, rebinding, or reinitializing `items` | Whole-collection structural mutation or exclusive access | Rejected (`AU3011` for a mutating method or `mut` argument, `AU3002`/`AU3001` for exclusivity, move, or rebinding as today) while any element loan is live |

| Operation on `table: dict[K, V]` | Classification | Effect on live entry loans of `table` |
| --- | --- | --- |
| `table[k]` shared read, `len`, `in`, `get`, `keys`, `values`, `items`, `copy`, passing to a bare parameter | Shared access | Allowed under shared entry loans; rejected under a mutable entry loan |
| `table[k] = value` (indexed assignment) | **Whole-collection structural mutation**: the key may be absent, insertion may rehash and relocate every entry | Rejected (`AU3011`) while any entry loan is live, even with a different literal key |
| `remove`, `update`, `clear`, `reserve`, `mut` argument or `mut self` method, `match mut`, move/rebind | Whole-collection | Rejected while any entry loan is live |
| Write through `view mut entry = table[k]` | Element-level write to the value slot; the key and its insertion position are untouched | Permitted; this is the only way to update a value under a live entry loan |

Disjointness is proven only between two element projections whose selectors
are both integer literals with different values, or two entry projections
whose selectors are both literals of the same key type with different values.
Every other pair of element projections on the same collection overlaps
conservatively, including `items[i]` versus `items[j]` for distinct locals,
and `items[i]` versus `items[i]` re-evaluated later (the second is a new
selection; only the first loan's captured value is known). This is the
conservative restriction ADR-0061 anticipates for runtime-known indexes.

```text
def main():
    mut pair = [1, 2]
    view mut left = pair[0]
    view mut right = pair[1]
    left += 10
    right += 20
    print(pair)

    mut names = ["ada", "grace"]
    view head = names[0]
    names.append("linus")    # AU3011: `append` may invalidate `head`
    print(head)
```

The dictionary asymmetry is deliberate: list indexed assignment replaces one
slot and is provably local, so it is an element-level write; dictionary
indexed assignment cannot be proven to hit an existing key, so it is
structural. A program that must update a present value under a live entry
loan uses the mutable entry view or the `match mut table.lookup(key)` form
of C1.

The alternative treats every mutation, including list indexed assignment,
as whole-collection and proves no disjointness. It needs no literal
comparison and no element-level write class, and it rejects the two-loan
example above. It is simpler by one rule and loses the one pattern that
matters for in-place algorithms (two mutable element views on known slots).
The recommendation costs one classification table that the Manual already
publishes per method, plus literal-only disjointness, which is the same
proof strength the place model already uses for fixed fields.

Invalidation is static. There is no runtime generation check on element
access beyond the existing loan-arena generation of the collection root: the
validator refuses any MIR that mutates a collection root or a non-disjoint
element while an element loan of that root is live, so a backend never sees
a dangling element descriptor. The runtime keeps only the checks it already
performs for root loans: exact-once registration and release, task locality,
and the tag recheck for union payloads.

### A4. Ownership through element views — Q4

**Recommend no new ownership operation**: element views obey ADR-0038's
aliasing rules unchanged ([ADR-0038:308–321](decisions/0038-place-based-loans-and-views.md)),
and moving an element out uses the existing transferring methods.

- A shared or mutable element view never yields ownership of a non-Copy
  element: `owned = view_binding` and `consume(view_binding)` are rejected
  (`AU3002`, "cannot move out of a view"), as for every view. A Copy pointee
  copies.
- `view_binding.clone()` produces an owned value when the element type is
  clone-safe; `tags[0].clone()` is the contextual-read form of the same
  operation and is the Approved Decision 5 example.
- Moving an element out uses `items.pop(index) -> T` (removes and shifts),
  `items.set(index, replacement) -> T` (replaces in place and transfers the
  old element), or `table.remove(key) -> Lookup[V]`
  ([collections.md:98, 110, 198](../docs/manual/collections.md)). For an
  optional element type, `items.set(i, None)` takes the element and leaves
  `None`. All three are whole-collection mutations for A3's purposes except
  `set`, which is an element-level write whose transferred result is owned;
  `set` is still rejected while a loan of the same or an unproven element is
  live.
- Reading a non-Copy element through a view into an owned local requires
  `.clone()`; the `AU3005` guidance keeps ADR-0014's three-way clone-safety
  classification ([ADR-0014:27–41](decisions/0014-map-literals-and-indexing.md))
  and adds `view` as the first suggestion when the use is a read.

```text
class Job:
    name: str
    tries: int64

def run_first(jobs: mut list[Job]) -> str:
    view mut job = jobs[0]
    job.tries += 1
    return job.name.clone()

def take_last(jobs: mut list[Job]) -> Job:
    return jobs.pop()
```

The alternative adds `items.take(index) -> T`, which would have to leave a
hole. Aura has no uninitialized element state and no default construction,
so `take` needs either an optional element type (`set(i, None)` already does
it) or a replacement argument (`set` already does it). It adds a name without
a new capability.

### A5. Iteration as element loans — Q5

Today bare and mutable list iteration use an older provenance model: loop
bindings carry `borrow_origin`/`frozen_places` state rather than a
`ViewBinding` ([sema.rs:817–826](../crates/aura-compiler/src/sema.rs)), the
source is frozen against overlapping mutation for the body
([ownership-and-borrowing.md:367–369](../docs/manual/ownership-and-borrowing.md)),
and `for x in mut items` commits each element through
`MutableIterationWriteback` on the iteration edge
([ADR-0038:545–560](decisions/0038-place-based-loans-and-views.md)).

**Recommend unifying list iteration with element loans**: `for x in items`
holds one shared loan of the collection root for the loop region and binds
`x` on each iteration as a shared element loan of `items[i]`; `for x in mut
items` holds one exclusive root loan and binds `x` as a mutable element
loan with immediate write-through; `for x in own items` is unchanged (the
collection moves into the loop-private source). The freeze becomes the
ordinary root loan; the loop binding becomes a first-class `ViewBinding`
whose region ends on every iteration edge, as ADR-0038 already requires for
views created in a loop ([ADR-0038:296–300](decisions/0038-place-based-loans-and-views.md)).

```text
def bump_all(jobs: mut list[Job]) -> None:
    for job in mut jobs:
        job.tries += 1
        view name = job.name
        print(name)

def main():
    view held = holder.jobs
    for job in held:
        print(job.name)
```

Consequences that must be pinned: an element view may be reborrowed from a
loop binding (`view name = job.name` above); a loop binding cannot be moved
out or stored; `items` cannot be structurally mutated in the body (as
today); the loop body may take shared element loans of *other* positions
under bare iteration (`view other = items[0]`) and may not under mutable
iteration; iteration through an existing view of a list (`for job in held`)
is a reborrow chain and composes with A6's returned views. Mutable iteration
writes through immediately rather than on the iteration edge. The only
observable difference from writeback is under a trap or cancellation inside
the body after a write: the write is now retained, which is exactly ADR-0038's
"a later trap ... cannot silently discard an earlier successful write"
([ADR-0038:305–309](decisions/0038-place-based-loans-and-views.md)). The
existing `list_mut_iteration` fixture output is unchanged; one new run-fail
fixture pins the retained write.

The alternative keeps loop provenance separate from views and leaves the
writeback action in place. It changes nothing observable and defers the
unification. It also leaves two overlap models in the checker (frozen places
and view loans) that Batch 2 must keep consistent by hand, and it prevents
`view` reborrows from loop bindings, which is the most common way an
application would take an element view. The recommendation retires
`MutableIterationWriteback` from the exit-action stack for lists; set
iteration keeps its shared-only rule, and Queue and Range iteration are
unchanged (they yield owned values, not places).

### A6. Returned element and entry views — Q6

**Recommend allowing an element or entry projection as the returned
expression of a `-> view [mut] T from origin` declaration, with the whole
origin root as the static footprint**:

```text
class Registry:
    tools: dict[str, Tool]

    def tool(self, name: str) -> view Tool from self:
        return view self.tools[name]

    def tool_mut(mut self, name: str) -> view mut Tool from self:
        return view mut self.tools[name]

def first(items: list[str]) -> view str from items:
    return view items[0]
```

ADR-0038 already locks the whole origin root when control flow may select
different projections ([ADR-0038:493–498](decisions/0038-place-based-loans-and-views.md));
an element selector is runtime data and therefore always a conservative
footprint. The caller's lock is on `registry` or `items`; the runtime token
records the exact selected element, as `BeginReturnedLoan`'s projection
selector does for field alternatives today
([native_codegen.rs:5178–5253](../crates/aura-compiler/src/native_codegen.rs)).
Exported callable metadata gains an `Element`/`Entry` footprint kind beside
the existing whole-root and fixed-projection kinds; imported interfaces and
the checked interface hash change accordingly. Absence traps at the `return
view` expression inside the callee, before any handoff, so an error before
`ReturnLoan` still "transfers nothing and ends the complete call-duration
loan set" ([ADR-0038:530–533](decisions/0038-place-based-loans-and-views.md)).

The Batch 1 stored-callable contract (Q22) extends without change: a
`Callable[def(items: list[T]) -> view T from items]` may return an element
view of its argument; a bound method returning `view ... from self` still
cannot be packed (C10 remains). Task targets still cannot return views.

The alternative defers returned element views and permits element loans only
as locals. It is smaller by the footprint kind and interface change, and it
leaves the reference agent's registry unable to expose a tool without
`remove`/reinsert, which is the limitation the Batch 1 checkpoint recorded
([16-batch-1-design-checkpoint.md:1435–1441](16-batch-1-design-checkpoint.md)).
The recommendation is the piece the reference agent needs first.

## B. Slice views

### B1. Spelling and pointee type — Q7

`items[a:b]` in value position is a fresh owned list under ADR-0040
([ADR-0040:36–44](decisions/0040-owned-vec-and-string-slices.md)); ADR-0038
states that a future indexed-view amendment "must use an explicit view form
and must not reinterpret the owned slice syntax as an alias"
([ADR-0038:56–62](decisions/0038-place-based-loans-and-views.md)), and
ADR-0061 keeps slice views and owned slices as separate operations
([ADR-0061:35–38](decisions/0061-collection-element-loans-and-slice-views.md)).

**Recommend that the `view` introducer applied to a slice expression selects
a range projection, and that the pointee of a slice view is the view-only
builtin type `Span[T]`**:

```text
def total(values: Span[int64]) -> int64:
    mut sum = 0
    for value in values:
        sum += value
    return sum

def main():
    mut items = [1, 2, 3, 4, 5]
    view window = items[1:4]
    print(len(window))
    print(total(window))
    print(total(items))
    copied = items[1:4]
    view mut tail = items[3:]
    tail[0] = 40
    print(items)
```

`view window = items[1:4]` is a shared range loan of `items` covering
positions 1, 2, and 3; `copied` is an owned `list[int64]` exactly as today;
`tail` is an exclusive range loan. Endpoints are evaluated once at creation
under the owned-slice normalization rules (half-open, one negative
normalization, no clamping, `AU4003` for a reversed or out-of-range pair,
[collections.md:155–159](../docs/manual/collections.md)). A range projection
overlaps every element projection whose literal index lies inside a
literal-endpoint range and overlaps every element projection conservatively
otherwise; it overlaps the whole root for structural mutation exactly as an
element loan does.

`Span[T]` is a **view-only pointee type**: it names what a slice view points
at and may appear only as the inferred pointee of a `view`/`view mut`
binding, as a bare or `mut` ordinary parameter type, and as the `T` of a
`-> view [mut] Span[T] from origin` result. It cannot be an owned local,
`own` parameter, ordinary `-> T` result, field, element, union member
(ADR-0052 excludes view types), task capture, or Queue payload; each of those
positions is `AU3010` (view escape) at the declaration or use. A whole
`list[T]` place supplies a `Span[T]` parameter by a call-duration whole-range
reborrow, so `total(items)` above needs no conversion, and an existing
`Span[T]` view supplies it by contained reborrow. There is no `Span[T]`
constructor. `Span[T]` is non-Copy, non-cloneable as a descriptor, and
non-Transfer, like every view.

| Pointee alternative | What it buys | What it costs |
| --- | --- | --- |
| `Span[T]` view-only pointee (recommended) | A small, closed operation set (B2); one new runtime representation (base, offset, length) touched only by span operations; honest hover and diagnostics; the natural read-only sequence parameter type | A new builtin type name; library signatures do not change in Batch 2, so `list[T]` parameters do not accept spans without `.to_list()` |
| `list[T]` pointee with a checker-tracked "range" restriction | Every existing `list[T]` shared API accepts a slice view | Every shared list operation in both runtimes must accept a sub-range form; the type claims `list[T]` while `append` is refused; the checker must special-case span bindings at each list operation |
| `Slice[T]` spelling of the same design | Python's `slice` vocabulary | Python's `slice` object is an index triple, not a view; `Span` avoids that confusion and matches C++/C# usage |

The first alternative is the one to reject on cost: it spreads the range
representation across the whole `list[T]` runtime surface on two backends.
The recommended design confines it.

### B2. Operations, mutation, nesting, and `str` — Q8

**Recommend a closed operation set for `Span[T]` consisting of the
length-preserving list operations, element loans, iteration, nested range
reborrows, and an explicit owned copy; `str` slice views are deferred**:

| Operation through a span view | Shared span | Mutable span | Semantics |
| --- | --- | --- | --- |
| `len(span)`, `span.len()` | yes | yes | Range length |
| `span[i]` contextual read, `view e = span[i]` | yes | yes | Element loan of the base list at `start + i`; bounds checked against the range |
| `span[i] = value` (Copy or non-Copy `T`), `view mut e = span[i]`, `span[i] op= rhs` (Copy `T`) | no | yes | Element-level write to the base list |
| `for x in span`, `for x in mut span` | yes | mut only | Element loans of the range, as A5 |
| `view inner = span[c:d]`, `view mut inner = span[c:d]` | yes | yes | Nested range reborrow; endpoints relative to the span; the parent is suspended by a mutable child as for every reborrow |
| `span[c:d]` in value position | yes | yes | Fresh owned `list[T]` copy (clone-safe `T`), as list slicing |
| `span.to_list()` | yes | yes | Fresh owned `list[T]` of the whole range (clone-safe `T`) |
| `span.reverse()`, `span.swap(i, j)`, `span.sort(...)` (both forms), `span.fill(value)` | no | yes | In-place, length-preserving; `sort` keeps its `Ord`/key contracts; `fill` requires Copy `T` |
| `value in span`, `span.index(value)`, `span.count(value)` | yes | yes | Equality-based searches over the range (`AU2008` without equality) |
| `span == other` | yes | yes | Element-wise equality against a `Span[T]` or `list[T]` of equal length |
| `append`, `insert`, `extend`, `pop`, `remove`, `clear`, `reserve`, `set` | no | no | Not members of `Span[T]`; `AU2001` (unknown member), with guidance naming the base list |

A span view holds a shared or exclusive range loan of its base for its
region; every A3 rule applies to the base collection. A mutable span view
suspends the base and any parent span exactly as a mutable reborrow does.

```text
def normalize(values: mut Span[float64]) -> None:
    mut total = 0.0
    for value in values:
        total += value
    for value in mut values:
        value = value / total

def main():
    mut samples = [1.0, 3.0, 4.0, 2.0]
    normalize(samples[1:3])
    view mut half = samples[:2]
    half.sort()
    print(samples)
```

`normalize(samples[1:3])` is the range form of the contextual mutable
element access in A1: a slice expression in a `mut Span[T]` argument position
is a call-duration exclusive range loan, not an owned copy; a slice
expression in a `Span[T]` bare argument position is a shared range loan. In
an owned position it remains a copy. The position decides, as it does for
`items[i]`.

`str` slice views are deferred. String slicing counts Unicode scalars and is
O(n) ([current-limits.md:113–117](../docs/manual/current-limits.md)); a text
span type would need a second text API (search, comparison, rendering,
f-string interpolation) and a byte-range representation with scalar-boundary
invariants. Owned `str` slices remain the maintained form; Batch 4's text
conveniences may reopen this with a concrete API list. `Array[T]` first-axis
slices likewise remain owned copies until Batch 10's strided views.

The alternative admits `str` spans now with the same range model over byte
offsets. It is the more complete surface, but it either exposes byte
positions (a new index domain for text) or pays the scalar scan at every
span creation, and it doubles the text operation set before Batch 4 has
selected it.

## C. App-facing dictionary lookup

### C1. Arm-scoped `lookup` — Q9

The Q23 ratification requires this checkpoint to "revisit an app-facing
`V | None` form of `dict.get`", to "review its access capability and result
lifetime together with dictionary-entry loans, and state how callers
distinguish a missing entry from a present None when V is itself optional",
and to "make its app-facing choice explicit alongside any retained tagged
lookup surface" ([16-batch-1-design-checkpoint.md:1078–1085](16-batch-1-design-checkpoint.md)).

What an application actually needs in the reference agent's `dispatch`
([main.au:78–85](../examples/agents/tool_runner/src/main.au)) is *access
without cloning*: today it must `remove`, call, and reinsert because
`dict.get` clones and a packed `Tool` is non-cloneable. `view tool =
registry[name]` (A1) traps on absence, and `name in registry` followed by the
index probes twice.

**Recommend one arm-scoped lookup operation on lists and dictionaries whose
result exists only as a `match` scrutinee and whose `Found` payload is an
element or entry loan**:

```text
def dispatch(request: ToolRequest, registry: dict[str, Tool]) -> Result[ToolResult, ToolError]:
    match registry.lookup(request.tool):
        case Lookup.Found(tool):
            return tool(value=request.value)
        case Lookup.Missing:
            return Result.Err(ToolError.UnknownTool(request.tool.clone()))

def bump(counts: mut dict[str, int64], key: str) -> None:
    match mut counts.lookup(key):
        case Lookup.Found(count):
            count += 1
        case Lookup.Missing:
            counts[key.clone()] = 1
```

`table.lookup(key)` and `items.lookup(index)` are compiler-known members
whose value is a **lookup outcome**, not a first-class value: it may appear
only as the scrutinee of a bare `match` or `match mut`; binding it, passing
it, returning it, storing it, or using `match own` on it is `AU2005` with
guidance naming `get` and `remove`. The arms use the existing `Lookup`
vocabulary: `Lookup.Found(name)` binds `name` as an arm-scoped shared entry
loan (bare match) or exclusive write-through entry loan (`match mut`), and
`Lookup.Missing` binds nothing. Coverage follows the ordinary two-case rule
(`AU2013` is not involved; `Lookup` arms use enum coverage). Guards read
through the shared loan; under `match mut`, the mutable loan begins after the
guard commits, as for mutable union payloads (A5 of Batch 1). The scrutinee
lock is the collection root for the arms, shared or exclusive by match mode;
inside a `Found` arm the A3 rules apply to the collection with one live loan.
The key is evaluated once; absence is the `Missing` arm rather than a trap.

The payload binding is a *view* of the stored value, so a present `None`
stays distinct from absence without any tag on the value: with `V = T |
None`, `case Lookup.Found(value)` binds a view of the stored union and
`value is None` narrows the view's pointee under the Batch 1 view rule
([16-batch-1-design-checkpoint.md:171–179](16-batch-1-design-checkpoint.md)).
This answers the Q23 present-None question by construction: presence is the
arm, the payload's own `None` is a narrowing test.

Lowering: the outcome is not an enum value and never reaches
`UnionInject`, `PatternWriteback`, or enum reconstruction. `match table.lookup(key)`
lowers to one probe producing a presence flag plus the element selector, a
conditional branch on the flag, and a keyed `BeginLoan` on the `Found` edge;
`match mut` uses the mutable form and writes through immediately, so
`MutableMatchWriteback` is not involved. The validator proves the loan is
dominated by the presence test, ends on every arm exit, and never escapes the
arm (the same arm-local rule as enum payload views,
[ADR-0038:264–268](decisions/0038-place-based-loans-and-views.md)).

| Alternative | Assessment |
| --- | --- |
| Arm-scoped `lookup` outcome (recommended) | One probe, no clone, present-None preserved, mutable form; a second-class outcome, consistent with second-class borrows; costs one compiler-known member and one guarded keyed loan lowering |
| Presence guard plus index (`if key in table: view v = table[key]`) | No new operation; two probes; the guarded index still carries a statically unprovable trap path; no combined mutable form beyond `view mut` |
| A `Lookup[view V]` value returned by a method | Requires a view-bearing generic enum, which ADR-0038 defers and ADR-0052 forbids for unions; a first-class view value would also need an escape rule for every destination |

`lookup` becomes a builtin member of `list[T]` and `dict[K, V]`; under the
clean-slate policy a user trait method named `lookup` implemented for those
targets is refused by the existing `AU2006` collision rule.

### C2. No app-facing `V | None` `dict.get` — Q10

**Recommend adding no `V | None` lookup and retaining `dict.get ->
Lookup[V]` and `list.get -> Lookup[T]` unchanged.** The review's findings:

- A `V | None` result cannot carry a loan: a union member may not be a view
  type ([ADR-0052:116–117](decisions/0052-anonymous-closed-union-types.md)),
  so any `V | None` lookup must clone, requires clone-safe `V`, and cannot
  retrieve a packed `Tool`. It would not serve the application case that
  motivated the review.
- An owned `V | None` flattens a present `None` into absence when `V` is
  optional, which Approved Decision 2 forbids wherever the distinction
  matters and which Q23 A's ratification says the follow-up must not
  "silently flatten".
- The app-facing surface that removes the clone is C1's `lookup`, the
  contextual index read of A1, and the entry loans; the owned tagged `get`
  remains the right generic API for callers that want an independent value.

The alternative adds `dict.get_optional(key) -> V | None` (cloned, owned)
restricted to non-optional `V` (`AU2010` when `V` normalizes to an optional
union), so that flattening can never occur. It is a convenience for the
`if value is not None:` shape with Copy or cheap values and it is
type-honest, but it duplicates `get` for one narrowing shape, still clones,
and adds a specialization restriction that generic code must work around.
It is listed so the choice is explicit, as Q23 requires.

## D. The lifetime-bearing callable and view dependency — Q11

ADR-0058 and ADR-0061 record one joint design: "the lifetime-bearing
callable type is one design shared by callable storage and collection
loans" ([ADR-0058:87–92](decisions/0058-first-class-callables-and-binding-contracts.md),
[ADR-0061:44–48](decisions/0061-collection-element-loans-and-slice-views.md)),
and "neither ADR may treat the other as independently responsible". Batch 1
delivered owned storage and a stored callable that may return a view of one
explicit ordinary argument (Q22), with stored loan environments and
captured-self origins deferred (C10). What exists today: `-> view [mut] T
from name` for named functions and stored contracts, encoded by parameter
ordinal with a static footprint ([closures.md:402–422](../docs/manual/closures.md),
[loans.rs:826–946](../crates/aura-compiler/src/sema/loans.rs)); loan
closures are local-only ([current-limits.md:85–92](../docs/manual/current-limits.md)).

**Recommend that Batch 2 fixes the origin and footprint vocabulary now and
that stored loan environments are owned by a dedicated lifetime-storage
checkpoint after Batch 3**, with these Batch 2 deliverables:

1. The returned-view origin footprint gains `Element`, `Entry`, and `Range`
   kinds beside whole-root and fixed-projection (A6, B1). Interfaces, MIR,
   the validator, and both backends encode them. Any later stored-loan
   design must reuse these kinds; it may not invent a second footprint
   vocabulary.
2. Callback sites that already accept loan closures and packed Shared
   callables (`map`, `filter`, keyed `sort`, `control.retry`) pass each
   element to the callback's bare parameter as a call-duration shared element
   loan, not a clone. No callback contract changes.
3. A stored `Callable[def(items: list[T]) -> view T from items]` or
   `Callable[def(values: Span[T]) -> view T from values]` is admitted under
   Q22's single-origin rule with the new footprint kinds.
4. Context managers (F) expose entry views through the existing `from self`
   returned-view contract on a *named* method; no stored manager callable is
   needed.

What remains deferred, and where it is owned: capture origins for loan
closures in storage, aggregate lifetime propagation (a field or list of loan
callbacks), captured-self returned views on packed bound methods,
receiver-loan binding and reborrow timing, and returned loan closures. This
checkpoint assigns them to a **lifetime-storage checkpoint scheduled after
Batch 3 and before Batch 6**, because Batch 6's decorators and Batch 9's
borrowing iterators both need stored loans and neither Batch 2 nor Batch 3
does. Until then the Batch 1 guidance stands: move or clone an owner into
storage, pass explicit state per invocation, or keep a loan closure local.

| Alternative | Assessment |
| --- | --- |
| Fix the vocabulary now; storage later (recommended) | Unblocks Batches 2 and 3; every new footprint kind is reused by the later design; no circular deferral because the owner is named |
| Design full lifetime-bearing storage in Batch 2 | Adds capture-origin annotations to `Callable[...]`, aggregate propagation, and a stored-loan validator model to the batch that must also deliver element loans; roughly doubles Batch 2 and delays Batch 3 |
| Never store loans; owned storage is final | Simplest; permanently forces clone/move for registries over borrowed sessions and rules out borrowing iterators, which Batch 9 requires |

## E. Custom initialization

### E1. `__init__` declaration, `self` authority, and the class call — Q12

Today a class is constructed only from its fields and per-instance defaults
([classes.md:39–72](../docs/manual/classes.md)); `__init__` has no
constructor meaning ([ADR-0059:11–14](decisions/0059-custom-initialization.md)).
ADR-0059's accepted decisions require the initializer's parameters to define
the class call, `self` to denote the object being initialized without being
supplied by the caller, complete initialization on every successful path,
rejection of reads of uninitialized fields and of partial escape, and no
dynamic attributes or inheritance ([ADR-0059:18–32](decisions/0059-custom-initialization.md)).
It leaves the `self` authority and its transition to ordinary ownership to
this design ([ADR-0059:53–58](decisions/0059-custom-initialization.md)).

**Recommend a bare-`self` initializer with initializer-specific authority,
ordinary parameter grammar, and no result annotation**:

```text
class Server:
    host: str
    port: int64
    label: str

    def __init__(self, host: str, port: int64 = 8080, *, prefix: str = "srv"):
        self.host = host
        self.port = port
        self.label = f"{prefix}:{port}"

def main():
    server = Server("0.0.0.0", port=9090)
    other = Server(host="127.0.0.1")
    print(server.label)
```

- The method must be spelled `def __init__(self, ...)` with bare `self`.
  `mut self`, `own self`, or no receiver is `AU2016`. Bare `self` is chosen
  because the initializer is the one context in which `self` is neither a
  shared receiver nor a mutable receiver of an existing value: it names a
  slot under construction. The authority is initializer-specific and does
  not change the shared meaning of bare `self` on ordinary methods.
- Inside the body, `self` is an **initializing place**: `self.field = value`
  initializes an uninitialized field (a move into the slot, no drop) or
  replaces an initialized one (evaluate the right side, drop the old value,
  move the new one); `self.field` may be read, projected, or lent only after
  it is definitely initialized (E4); `self` as a whole may be read, passed,
  lent, matched, or used as a method receiver only after every field is
  definitely initialized, at which point it behaves as a `mut self` receiver
  for the rest of the body. `self` cannot be moved (`own` argument, owned
  binding, return, capture by value) at any point: the slot belongs to the
  caller (`AU3004`).
- Parameters use the ordinary grammar: bare, `mut`, and `own` capabilities,
  defaults, the keyword-only `*` boundary, and the class's type parameters.
  `__init__` cannot declare its own type parameters (`AU2016`); the class
  call infers the class's parameters from the initializer arguments or takes
  them explicitly (`Box[int64](value=1)`), and every class parameter must be
  determined (`AU2002`).
- A result annotation other than none or `-> None` is `AU2016`. `return`
  without a value is permitted only where every field is definitely
  initialized; fallthrough is checked the same way. `return value` is
  `AU2016`.
- `__init__` is never callable as a method: `server.__init__(...)` and
  `Server.__init__` as a value are `AU2005`. It is not a trait member; an
  enum has none. Argument binding follows ADR-0015: supplied arguments in
  source order, omitted parameter defaults in declaration order, then E2's
  field defaults, then the body.
- `copy class` may declare `__init__`; the fields remain Copy.

The class call `Server(...)` is checked as a call of the initializer's
contract with the receiver slot removed; hover, signature help, and the
checked interface expose that contract. The object is constructed in the
call's result slot: both backends allocate the class storage first, pass it
to the initializer as the `self` place, and return the completed value from
the call expression. There is no intermediate owned temporary to move.

The alternative spells the receiver `mut self`, treating the initializer as
an ordinary mutable method over a slot the compiler pre-allocates. It reuses
the mutable-receiver machinery and hover renders it as a familiar mode, but
it is a lie for the first half of the body (a `mut self` receiver is a
complete value), it invites `self.method()` before completion, and it
diverges from the approved `__init__(self, ...)` spelling.

### E2. Field defaults and the field-based constructor — Q13

**Recommend that declaration defaults are evaluated before the body and
that a class with `__init__` has no field-based class call**:

```text
class Counter:
    value: int64 = 0
    name: str

    def __init__(self, name: own str):
        self.name = name

def main():
    counter = Counter("hits")
    print(counter.value)
```

Fields with a declaration default are initialized fresh, in declaration
order, after argument binding and before the first body statement; they are
definitely initialized at the body's entry and the body may leave them alone
or replace them. Fields without a default start uninitialized and must be
assigned on every path. The default is an ordinary field default under the
existing rules (checked in declaration context, evaluated afresh per
construction, unable to call a user function in the current compiler,
[classes.md:25, 326–328](../docs/manual/classes.md)); replacing it in the body
drops the default value, and that cost is visible in the declaration.

When `__init__` is declared, the field-based constructor is not available:
`Counter(name="hits", value=3)` binds against the initializer and reports
`AU2004` for `value`. A class that wants both shapes declares a named
factory. Without `__init__` the field constructor is unchanged.

The alternative evaluates no field defaults when `__init__` exists and
requires the body to assign every field. It never drops a value the body
replaces, but a declared default then means nothing for such a class, and
adding an initializer to an existing class silently changes which fields are
required. Retaining defaults keeps the field declaration's meaning stable.

### E3. Visibility of `__init__` across modules — Q14

Today a cross-module constructor may initialize only public fields, so a
private field on a publicly constructed class needs a default
([classes.md:94](../docs/manual/classes.md)).

**Recommend that `__init__` follows ordinary method visibility and that its
visibility governs the class call from other modules**:

```text
public class Connection:
    handle: int64
    label: str

    def __init__(self, handle: int64):
        self.handle = handle
        self.label = f"conn-{handle}"

    public def open(path: str) -> Result[Connection, io.Error]:
        handle = try acquire(path)
        return Result.Ok(Connection(handle))
```

`Connection(...)` is callable inside its module; another module sees a
public class whose initializer is private and must use `Connection.open`.
`public def __init__` makes the class call available wherever the class is
visible. The initializer body always runs in its defining module, so it may
assign private fields regardless of the caller; the private-field rule for
field-based construction is unchanged for classes without `__init__`.

This gives the named-factory pattern from Approved Decision 3 a way to be
the *only* construction route, which is what a fallible resource type needs
(E5). The alternative makes every `__init__` callable wherever the class is
visible and ignores a `public` marker on it. It has one fewer rule and no
way to force external construction through a factory; a class would need to
hide behind a private class plus a public wrapper.

### E4. Definite initialization — Q15

**Recommend flow-sensitive per-field initialization state, using the
existing move-checking join and loop rules**
([ownership-and-borrowing.md:240–246](../docs/manual/ownership-and-borrowing.md)):

Each field of `self` is `Uninit`, `Init`, or `MaybeInit`. The transfer
rules are:

| Statement in `__init__` | Rule |
| --- | --- |
| `self.f = value` with `f` `Uninit` | Initialize: `f` becomes `Init`; no drop |
| `self.f = value` with `f` `Init` | Replace: evaluate the right side, drop the old value, move the new one (existing assignment semantics) |
| `self.f = value` with `f` `MaybeInit` | `AU3012`: the compiler cannot know whether to drop |
| `self.f.g = value`, `view x = self.f`, `self.f` in any read, receiver, or argument position | Requires `f` `Init`; else `AU3012` |
| `own` use of `self.f` (a consuming argument or `match own self.f`) | Requires `Init`; `f` returns to `Uninit` and must be reassigned before completion (the ordinary partial-move state machine) |
| `self` as a whole in any read, receiver, argument, capture, `match`, or `with` position | Requires every field `Init`; else `AU3012` ("partially initialized `self` cannot escape") |
| Branch join | A field is `Init` only if `Init` on every incoming path; `Uninit` on all is `Uninit`; otherwise `MaybeInit` |
| Loop | Fixed point over entry and back edges; a first initialization inside a repeatable loop yields `MaybeInit` after the loop unless the loop is proven to execute exactly once by the limited constant reasoning already used for moves |
| `return`, fallthrough | Every field `Init`; else `AU3012` naming the missing fields and the exiting path |
| A trap, cancellation, or `try` propagation is impossible (E5) | Partial cleanup under G1 |
| A call to a helper receiving `self.f` (bare or `mut`) before completion | Permitted when `f` is `Init`; the helper sees an ordinary place of the field's type and cannot reach `self` |

```text
class Buffer:
    data: list[uint8]
    capacity: int64
    label: str

    def __init__(self, capacity: int64, *, label: str = "buf"):
        if capacity < 0:
            self.capacity = 0
        else:
            self.capacity = capacity
        self.data = list[uint8].with_capacity(self.capacity)
        self.label = label
        self.reset()

    def reset(mut self) -> None:
        self.data.clear()
```

`self.capacity` is `Init` at the join and may be read to build `data`;
`self.reset()` is legal because it follows the last initialization. Nested
fields follow the same rule: initializing `self.profile` initializes the
whole projection; a partial assignment `self.profile.name = ...` before
`self.profile` is `Init` is `AU3012`. Class patterns (Batch 4) will inherit
these states through ADR-0063's access model; nothing here decides positional
exposure.

The alternative is a linear-prefix rule: every field must be assigned by
straight-line statements at the top of the body, before any branch, loop,
call, or read of `self`. It needs no dataflow beyond statement order, and it
rejects the `capacity` example and every computed field that depends on a
validated input. Approved Decision 3 asks for validation and computed
fields, so the flow-sensitive rule is required.

### E5. Named fallible factories and partial construction — Q16

Named factories already exist as associated methods
(`def zero() -> Counter`, [classes.md:113–114](../docs/manual/classes.md)),
and `Result` returns with `try` already run active cleanups on early return
([functions.md:212–221](../docs/manual/functions.md)).

**Recommend that fallible construction is a named associated function
returning `Result[Class, Error]`, that `__init__` cannot return `Result`,
and that the factory's partial state is cleaned up by ordinary owned-local
drops plus G1's initializer mechanism**:

```text
class Pool:
    primary: fs.File
    replica: fs.File

    def __init__(self, primary: own fs.File, replica: own fs.File):
        self.primary = primary
        self.replica = replica

    public def open(first: str, second: str) -> Result[Pool, io.Error]:
        mut primary = try fs.open(first)
        match fs.open(second):
            case Result.Ok(replica):
                return Result.Ok(Pool(primary, replica))
            case Result.Err(error):
                primary.close()
                return Result.Err(error)
```

- `__init__` has no `Result` result (E1); a typed failure inside it is
  impossible, so the initializer's partial state is exposed only to a trap
  or cancellation, which G1 cleans up exactly once. This is ADR-0059's
  "initializer returning a fallible construction result is not part of the
  initial accepted surface".
- In a factory, resources acquired before a failing `try` are ordinary owned
  locals. They are dropped on the early return under the existing rules.
  Dropping a builtin resource value releases its runtime-owned host state;
  the language-level `close` of a user resource class runs only through
  `with` or a manager (F). A factory that must run `close` on the failure
  path writes it explicitly, as `open` does above. Whether every builtin
  resource wrapper releases its host handle when its last owner is dropped
  without `with` is verified by a fixture in phase 3a rather than assumed
  here.
- A managed `with` binding cannot be released into an owned destination.
  The existing rule stands: "the managed value cannot be moved out in a way
  that prevents cleanup" ([with_resource_move_out_field_rejected.diag](../crates/aura-compiler/tests/fixtures/check-fail/with_resource_move_out_field_rejected.diag)).
  A factory therefore acquires with plain locals or with a manager whose
  `__exit__` is idempotent after an explicit disarming call, not with `with`.

The alternative admits `def __init__(self, ...) -> Result[None, E]` now,
with the class call becoming a `Result[Class, E]` expression. It removes the
factory boilerplate for the common case, but every class call site then
needs `try` or a match, the field-based and initializer forms diverge in
result type, G1 must additionally disarm on the `Err` path, and ADR-0059
explicitly excludes it from the initial surface. It is listed for
completeness and recommended against for Batch 3; it can be revisited once
the cleanup mechanism has shipped.

## F. Typed context managers

### F1. The protocol — Q17

`with` today manages builtin resources and non-generic user classes that
declare exactly `close(mut self) -> None` ([statements.md:400–402](../docs/manual/statements.md),
[sema.rs:15095–15135](../crates/aura-compiler/src/sema.rs)); the shape is
matched structurally, by name and signature, not through a nominal trait.

**Recommend a shape-matched dunder protocol, `__enter__` and `__exit__`,
with the existing `close(mut self) -> None` shape retained as the implicit
manager whose entry is empty and whose exit is `close`**:

```text
class Transaction:
    conn: Connection
    committed: bool = false

    def __init__(self, conn: own Connection):
        self.conn = conn

    def __enter__(mut self) -> None:
        self.conn.begin()

    def __exit__(mut self) -> None:
        if not self.committed:
            self.conn.rollback()

    def commit(mut self) -> Result[None, DbError]:
        try self.conn.commit()
        self.committed = true
        return Result.Ok(None)

def transfer(conn: own Connection) -> Result[None, DbError]:
    with tx = Transaction(conn):
        try tx.conn.write("debit")
        try tx.commit()
    return Result.Ok(None)
```

A class is a **manager** when it declares `__exit__(mut self)` with no
ordinary parameters and one of the result shapes in F5, optionally with
`__enter__(mut self)` with no ordinary parameters and one of the entry
shapes in F2. A class that declares `close(mut self) -> None` and neither
dunder is a manager whose entry does nothing and whose exit is `close`.
Declaring both `close` and `__exit__` does not make `close` the exit; the
dunder wins and `close` is an ordinary method. Builtin resources keep their
documented cleanup. The dunders are ordinary methods: they may be called
explicitly, subject to their receiver capability (H1), and a manager may be
`public` or private like any class.

Shape matching is chosen over a nominal `Manager` trait because it is what
`close(mut self)` already does, it is the Python shape, it needs no `impl`
block for the common case, and traits remain nominal for everything else
([generics-and-traits.md:5](../docs/manual/generics-and-traits.md)). The
alternative introduces a builtin trait (`impl ContextManager for
Transaction:` with `enter`/`exit` methods). It composes with generic bounds
(`def run[M: ContextManager](m: own M)`) and is more explicit, but it is a
second mechanism beside the shape-matched `close`, it needs an associated
type for the entry value, and a generic bound over managers is not a Batch 3
need. If a later batch wants manager-generic code, a builtin trait can be
layered over the same dunder shapes.

### F2. Entry result shapes — Q18

**Recommend two entry shapes — unit and a returned view of `self` — with
typed failure placed in the factory that produces the manager, not in
`__enter__`**:

| `__enter__` declaration | Binding created by `with M(...) as name:` or `with name = M(...):` | Use |
| --- | --- | --- |
| absent, or `-> None` | `name` is the managed manager binding (today's meaning) | Resources, locks, transactions, scoped configuration |
| `-> view T from self` | `name` is a shared view whose origin is the managed manager (F4); the manager itself is a hidden managed place | A reader exposing its buffer, a guard exposing the protected value |
| `-> view mut T from self` | `name` is an exclusive write-through view of the managed manager | A lock guard exposing mutable protected state |

```text
class Guard:
    value: list[int64]

    def __enter__(mut self) -> view mut list[int64] from self:
        return view mut self.value

    def __exit__(mut self) -> None:
        print("released")

def main():
    with Guard(value=[1, 2]) as values:
        values.append(3)
        print(len(values))
```

`__enter__` may not return an owned value: an owned entry value would have
to be stored somewhere other than the manager, and Approved Decision 4's
"scoped resource or view ... cannot escape" is exactly a returned view of
the manager. `__enter__` may not return `Result`: a `Result` carrying a view
is a view-bearing aggregate ([ADR-0038:372–375](decisions/0038-place-based-loans-and-views.md)),
and a unit `Result[None, E]` entry would need an implicit `try` at the
header. Entry may still fail by trap or cancellation, which is the failure
ADR-0060 requires to leave earlier entries cleaned up (F3); typed failure
belongs to the expression that produces the manager (`with tx = try
conn.begin():`), which `try` already handles.

The alternative also admits `__enter__(mut self) -> Result[None, E]` with
an implicit `try` at the `with` header, requiring the enclosing function to
return a compatible `Result`. It expresses "acquire may fail" on the
manager, but it hides a propagation point inside a statement that otherwise
has none, cannot combine with an entry view, and duplicates what a factory
expresses without new rules.

### F3. Generic managers and multiple header items — Q19

**Recommend admitting generic manager classes and a comma-separated list
of header items, entered left to right, each registering its exit only
after its own entry succeeds, exited in reverse**:

```text
with-statement
    = "with", with-item, { ",", with-item }, ":", NEWLINE, suite ;

with-item
    = identifier, "=", expression
    | expression, "as", identifier ;
```

```text
def copy_all(source: str, target: str) -> Result[None, io.Error]:
    with input = try fs.open(source), output = try fs.create(target):
        text = try input.read_all()
        try output.write_all(text)
    return Result.Ok(None)
```

If `fs.create` fails, `try` propagates after `input` has entered, so
`input`'s registered exit runs; `output` never registers. A trap or
cancellation inside the second manager's `__enter__` behaves the same way.
Each item's binding is visible to later items' expressions. Nested
`with` statements remain equivalent; the list form is sugar for nesting
with one indentation level, and lowering emits the same `PushCleanup`
sequence in order ([mir.rs:11199–11231](../crates/aura-compiler/src/mir.rs)).

Generic user managers are admitted: the restriction "`with` does not yet
support generic resource types" ([sema.rs:15095–15103](../crates/aura-compiler/src/sema.rs))
is removed because the specialization is static and the exit call is an
ordinary specialized method call. `with box = Box[int64](value=1):` is
rejected only if `Box[T]` is not a manager.

The alternative keeps single-item headers and requires nesting. It changes
no grammar and it forces one indentation level per resource, which is the
ergonomic complaint the roadmap names ("multiple resources"). The multi-item
grammar is the Python shape and its evaluation order is fully determined by
the existing sequential registration rule.

### F4. Header temporaries as managed places and entry views — Q20

ADR-0038 excludes "a temporary that would need hidden stable storage" from
view origins ([ADR-0038:233](decisions/0038-place-based-loans-and-views.md)),
and ADR-0060 requires this design to expose a scoped entry view from a
manager constructed in the header "through a valid origin ... do not assume
an implicit temporary exemption" ([ADR-0060:47–50](decisions/0060-typed-context-managers.md)).

**Recommend that every `with` item's manager value occupies a managed
place — the named binding in the `name = expression` and unit-entry `as`
forms, or a compiler-named hidden managed binding when `as name` binds an
entry view — and that this managed place is the entry view's origin**:

```text
with Guard(value=[1, 2]) as values:
    values.append(3)
```

lowers as if written

```text
with __with_0 = Guard(value=[1, 2]):
    view mut values = __with_0.__enter__()
    values.append(3)
```

where `__with_0` is a fresh managed binding that source code cannot name.
This is not a temporary exemption: `with` already consumes its expression
into "a fresh mutable managed binding" ([statements.md:400](../docs/manual/statements.md))
with a real slot and a fresh storage generation, and `check_with` already
creates that binding ([sema.rs:4310–4335](../crates/aura-compiler/src/sema.rs)).
The design makes the slot the origin whether or not the user names it. The
entry view's region is bounded by the body; the manager is locked for the
body by that view (shared or exclusive by the entry kind), so the body
cannot reach the hidden manager at all in the view form, and cannot move,
rebind, or close the named manager while its entry view is live in the
named form. `__exit__` runs after the view's region ends, which the ordered
exit-action stack guarantees because the loan end is pushed after the
manager's registration ("a view into a managed resource ends before
`close(mut self)` cleanup", [ADR-0038:302](decisions/0038-place-based-loans-and-views.md)).

The alternative rejects entry views from `as` on header expressions and
requires a named manager plus an explicit view statement in the body
(`with guard = Guard(...):` then `view mut values = guard.__enter__()`). It
adds no hidden binding, but it makes `__enter__` an ordinary method the
user must call, which loses the protocol's guarantee that entry precedes the
body, and it leaves the manager reachable while its view is live only by the
ordinary loan rules, which is acceptable but noisier. The hidden managed
binding is the smaller user-facing surface and reuses the existing slot.

### F5. Exit results, failure precedence, and cleanup failure — Q21

ADR-0060 requires the body failure to stay primary with cleanup failures
attached, no silent suppression through an exit return value, and a
specified propagation for cleanup-only failure ([ADR-0060:29–31, 57–63](decisions/0060-typed-context-managers.md)).

**Recommend `__exit__(mut self) -> None` or `__exit__(mut self) ->
Result[None, E]`, with a typed exit failure propagating from the `with`
statement on a normal exit path and never replacing an in-flight failure**:

| Exit path from the body | `__exit__` returns `None` | `__exit__` returns `Result.Ok(None)` | `__exit__` returns `Result.Err(e)` | `__exit__` traps |
| --- | --- | --- | --- | --- |
| Fallthrough, `return`, `break`, `continue` | continue | continue | The `with` statement propagates `e` as if by `try`: the enclosing function must return `Result[_, E2]` with `E2 == E` or an applicable `From` (`AU2017` otherwise); a `return value` expression is evaluated before exit and its value is discarded when exit fails | Ordinary trap teardown; later registrations still drain |
| `try` propagation of a body error | continue propagating | continue propagating | `e` is dropped after the exit call; the body's typed error remains the function's result; the drop is not a suppression of the body failure | The trap becomes primary over the typed error, as today for a trap during cleanup |
| Trap or cancellation teardown | continue draining | continue draining | `e` is rendered and attached as related information to the primary diagnostic; teardown continues | Attached as related information; the original diagnostic remains primary (existing rule, [statements.md:411](../docs/manual/statements.md)) |

The value of a `return` expression is computed before the exits run
(today's order, [statements.md:206](../docs/manual/statements.md)). An exit
`Err` on the normal path is a typed outcome that the caller can handle,
which is what a transaction rollback or a file flush needs. Discarding an
exit `Err` while a body `Err` propagates is the only composition that keeps
one typed result per function without inventing an error-pair type; the
body failure stays primary, which is the ADR's rule. A manager that must
report cleanup failure alongside a body failure exposes an explicit method
the body calls (`commit`), as `Transaction` does in F1.

The `AU2017` static check is the same shape as `try`'s: a `with` over a
manager whose exit may fail, inside a function that does not return a
compatible `Result`, is rejected at the `with` keyword with the exit's error
type named. A `None`-returning exit adds no requirement. `close(mut self) ->
None` managers keep their existing behavior; a `close(mut self) ->
Result[None, E]` method does not make a class a manager (the shape is
exact), so no existing program changes meaning.

The alternative admits only `-> None` exits and makes every cleanup failure
a trap (`AU4005`). It is one column of the table and needs no propagation
rule, but it turns a rollback or flush failure into task termination, which
is disproportionate for the transaction and file cases the roadmap names.

### F6. Cancellation during exit — Q22

**Recommend that exits run with cancellation still pending and no
shield**: when cooperative cancellation is observed inside the body at a
scheduler-aware wait, teardown drains the exit-action stack; each
`__exit__` runs on the still-live frame with the task's cancellation flag
set, so any scheduler-aware wait inside the exit observes cancellation
immediately and returns its documented `Cancelled` outcome. An exit that
needs bounded host work under cancellation performs it through the
non-blocking or already-cancellation-aware operations its resource
documents. `TaskGroup`'s existing `cancel_before_cleanup` behavior is
unchanged ([mir.rs:8152–8168](../crates/aura-compiler/src/mir.rs),
[mir_runtime.rs:3687–3722](../crates/aura-compiler/src/mir_runtime.rs)).

The alternative grants each exit a fixed grace period during which
cancellation is masked. It makes cleanup more reliable for I/O-heavy exits,
but ADR-0060 forbids choosing a duration policy here, a mask is a new
scheduler contract, and a masked exit that blocks defeats cancellation for
the whole group. The recommendation is the bounded interaction ADR-0060
asks for: the exit runs, exactly once, and cannot wait indefinitely.

## G. One partial-construction cleanup mechanism

### G1. Extending the ordered exit-action stack — Q23

The roadmap requires "one partial-construction cleanup mechanism extending
ADR-0038's ordered exit-action stack for initializer failure, multi-resource
entry failure, and failed decodes", "not three independent initializer,
manager, and decoder mechanisms" ([roadmap:157–162](14-priority-roadmap.md);
[ADR-0059:38–41](decisions/0059-custom-initialization.md),
[ADR-0060:51–53](decisions/0060-typed-context-managers.md),
[ADR-0062:45–48](decisions/0062-typed-serialization-validation-and-schemas.md)).
The stack today contains loan ends, closure environment drops, mutable
writebacks, match/iteration reconstruction, and resource cleanup
([ADR-0038:545–560](decisions/0038-place-based-loans-and-views.md),
[execution-model.md:146–153](../docs/manual/execution-model.md)); MIR
expresses resource cleanup as `PushCleanup`/`PopCleanup` on a place
([mir.rs:780–786](../crates/aura-compiler/src/mir.rs)), the interpreter
keeps a `cleanup_stack` of places ([mir_runtime.rs:3278–3345](../crates/aura-compiler/src/mir_runtime.rs)),
and the direct backend registers thunks through
`aura_direct_register_cleanup`/`unregister` ([native_runtime.rs:8229–8253](../crates/aura-compiler/src/native_runtime.rs),
[native_codegen.rs:1981–2010](../crates/aura-compiler/src/native_codegen.rs)).

**Recommend generalizing the cleanup instructions to typed exit actions and
adding exactly two action kinds, `DropInitialized` and `ManagerExit`, with
static disarming**:

```text
PushExitAction { action }
PopExitAction { action, run: bool }

ExitAction =
    ResourceClose { place }                       # today's PushCleanup
  | ManagerExit { place, exit_result: ExitResult } # __exit__ on a manager place
  | DropInitialized { place }                     # a field or prefix under construction
```

- **Initializer failure.** The class call allocates the result slot and
  enters `__init__` with an empty action set. Each first initialization of
  `self.f` pushes `DropInitialized { self.f }`; a replacement pushes
  nothing (the slot already has an action). At every completion exit
  (`return` without value, fallthrough), lowering emits `PopExitAction {
  run: false }` for every field action in reverse order: the object is
  whole and belongs to the caller's slot. A trap or cancellation before
  completion drains the suffix, dropping initialized fields in reverse
  initialization order exactly once; the uninitialized fields are never
  touched. An `own` use of an initialized field pops its action (`run:
  false`) and its reinitialization pushes a new one.
- **Multi-resource entry failure.** Each `with` item pushes `ManagerExit`
  after its `__enter__` returns; a later item's failing `try`, trap, or
  cancellation drains the suffix, so earlier managers exit in reverse. This
  is the existing registration order with a typed action.
- **Failed decodes (Batch 6).** A generated decoder constructs into an
  uninitialized slot exactly as `__init__` does, pushing `DropInitialized`
  per field or per constructed collection prefix; a validation failure at
  field `k` drains fields `0..k`. Batch 6 adds no action kind.
- **Loans.** `EndLoan` stays a separate instruction; loan ends of a view
  into a manager or into a field under construction are ordered after the
  corresponding registration by the same push order, as today.

The validator proves for every function: each push is dominated by the
producing operation (the field assignment, the `__enter__` call, the
resource creation); each path from a push to a region exit executes exactly
one pop or drains it; no pop names an action that is not on top of the
stack; no `run: false` pop occurs on a path where the object is incomplete;
and no `DropInitialized` outlives the initializer's frame. These are the
same shape as the existing loan proofs ([mir.rs:8136–8168](../crates/aura-compiler/src/mir.rs)).
Both backends consume the same actions; the interpreter's `cleanup_stack`
becomes a stack of typed actions, and the direct backend's registration
thunks gain a drop-field thunk kind. Every refusal is pinned at `run_mir`
and `emit_host_native_object`.

| Alternative | Assessment |
| --- | --- |
| Two typed actions on the existing stack (recommended) | One drain path for every abnormal exit; the validator's existing dominance/exact-once proofs extend; Batch 6 reuses `DropInitialized` unchanged |
| Runtime-side construction scope object | The runtime tracks which fields are initialized dynamically and drops them on failure; no static proof; both backends need a per-object initialization bitmap at run time; cancellation and trap paths must find the object |
| Separate per-feature mechanisms | Rejected by the roadmap |

### G2. Cancellation versus forced frame reset — Q24

ADR-0060 promises cleanup under cancellation; ADR-0038 states that full
source exit actions run only while the generated frame is intact, and that
after a forced reset host containment "may release opaque loan descriptors,
runtime cells, and ledger registrations exactly once, but it must not
execute arbitrary Aura cleanup, reconstruction, or writeback against a
destroyed frame" ([ADR-0038:562–569](decisions/0038-place-based-loans-and-views.md)).
The direct runtime's forced-exit callback is documented the same way
([native_runtime.rs:3760–3768](../crates/aura-compiler/src/native_runtime.rs)),
and the Manual describes scheduler abandonment as a containment path that
"does not invoke arbitrary Aura cleanup thunks" ([execution-model.md:410–419](../docs/manual/execution-model.md)).
The Batch 1 checkpoint left this conflict open ([16-batch-1-design-checkpoint.md:1673](16-batch-1-design-checkpoint.md)).

**Recommend reconciling by scope, not by weakening either record**: the
cleanup-under-cancellation promise applies to *cooperative cancellation
observed by the task itself*, which exits through the language cleanup
machinery on a live frame; forced frame reset occurs only under scheduler
abandonment, which is not cancellation of a running body but containment
after the scheduler has stopped. Concretely:

1. A task that observes cancellation at a scheduler-aware wait, a `cancelled()`
   test, or a cancellation outcome unwinds through its own exit actions:
   `ManagerExit`, `DropInitialized`, `ResourceClose`, loan ends, and closure
   drops all run on the live frame (F6). This is the promise ADR-0060 makes,
   and it is the only cancellation an Aura program can observe.
2. Scheduler abandonment after a forced reset releases *runtime-owned*
   state exactly once: registered cleanup thunks are dropped, not invoked
   (`DirectCleanupRegistration`'s drop, [native_runtime.rs:145](../crates/aura-compiler/src/native_runtime.rs));
   loan descriptors and ledger entries are released; host handles owned by
   runtime values are closed by their runtime drop. No `__exit__`, `close`,
   or field drop written in Aura runs. Static lowering must have arranged
   every ordinary write-through and every source cleanup before that
   boundary, which G1's ordering does.
3. The Manual states this boundary in the context-manager chapter: a
   manager's exit is guaranteed on every language-level exit, including
   cooperative cancellation, and is not guaranteed under scheduler
   abandonment, which programs avoid by structured `TaskGroup` scopes. No
   manager may be documented as running under abandonment.

The alternative runs `__exit__` on the host side after the reset by
snapshotting the manager value into a runtime cell at registration. It
would execute Aura code against a frame that no longer exists, which the
direct backend cannot do safely across Cranelift frames; ADR-0038 excludes
it and this checkpoint does not reopen it. A second alternative pins every
frame that holds a registration so that it can never be reset; it turns
containment into a hang.

## H. Resource capability policy

### H1. Uniform enforcement of builtin receiver capabilities — Q25

The H2 open item ([work record:184–195](../work/2026-09-19-batch-1-phase-2.md)):
the checker accepts `match child.stdin(): case process.Pipe as pipe:
pipe.close()` although the binding is a shared view and the builtin table
declares `close` as `mut self`; the shared MIR validator refuses the lowering
on both backends. The table is `BuiltinMember::receiver_passing`
([call.rs:2567–2600](../crates/aura-compiler/src/call.rs)), which lists
`QueueClose`, `FileClose`, `TcpListenerClose`, and the other resource closes
beside `VecPush` and `MapSet`; the checker consults it only for retained
receiver/argument overlap ([loans.rs:3543–3560](../crates/aura-compiler/src/sema/loans.rs))
and view-value passing ([loans.rs:2236–2245](../crates/aura-compiler/src/sema/loans.rs)),
while user methods go through `require_mutable_receiver`
([capabilities.rs:788–810](../crates/aura-compiler/src/sema/capabilities.rs)).
The Manual documents per-member capabilities: file writes, flush, and close
require a mutable receiver while reads do not ([filesystem.md:120](../docs/manual/filesystem.md));
pipe write/flush/close and child kill/terminate require mutable places
([process.md:268](../docs/manual/process.md)). The reference agent calls
`events.close()` on a *bare* `Queue[str]` parameter ([main.au:95–100](../examples/agents/tool_runner/src/main.au)),
which the table's `mut` declaration for `QueueClose` would reject if it
were enforced.

**Recommend enforcing the builtin receiver table at every call site through
`require_mutable_receiver`, after auditing the table so that operations on
Copy runtime handles are shared**:

- Every builtin member declared `mut self` requires a mutable place for its
  receiver: a `mut` local, a `mut` parameter or receiver, a mutable view, a
  managed `with` binding, or an owned binding under H2. A shared view, a
  bare parameter, a shared match binding, or an immutable owned local is
  `AU3003`, with the same message user classes receive. `match child.stdin():
  case process.Pipe as pipe: pipe.close()` is therefore refused by the
  checker with `AU3003` and guidance naming `match own`, which agrees with
  the validator and with the Manual's example ([process.md:118–133](../docs/manual/process.md)).
- The audit: `Queue[T]` is a Copy handle to shared runtime state
  ([ownership-and-borrowing.md:31–34](../docs/manual/ownership-and-borrowing.md)),
  so a `mut` requirement on `put`, `try_put`, or `close` is unenforceable —
  any copy of the handle is a fresh mutable place — and misdescribes the
  operation. `QueuePut`, `QueueTryPut`, and `QueueClose` become shared
  receivers; repeatable `Task[T]` operations likewise. Every operation that
  mutates the *value* (list, dict, set, Array mutators) or a non-Copy host
  resource (file, socket, listener, stream, child, pipe, supervisor writes,
  flushes, closes, kills, starts, stops) keeps its documented `mut`
  requirement. `read_all`/`read_bytes` stay shared as documented.
- The audited table is the single source for the checker, the MIR
  validator's call checks, analysis hover, and the Manual's API tables;
  the phase adds a characterization test that every documented receiver
  mode equals the table entry.

The cost is clean-slate breakage where existing programs call a `mut`
member on an immutable owned local (`pipe = ...; pipe.close()` becomes
`mut pipe = ...`) or through a shared binding; fixtures, tutorials, and
examples are updated in the same change family and the checker's message
names the fix. The alternative leaves enforcement at explicit sites and
makes the validator tolerant of the checker's acceptance, which would let a
shared view close a resource on both backends; it is the unsound direction
and is listed only to be rejected. A second alternative reclassifies every
builtin `close` as a shared operation on interior host state, so the
open-item program compiles unchanged; it contradicts the Manual's documented
capabilities for files and pipes and would make `with`'s `close(mut self)`
contract for user classes stricter than the builtins'.

### H2. Owned pattern and loop bindings are mutable places — Q26

With H1 in force, `match own child.stdin(): case process.Pipe as pipe:
pipe.close()` needs `pipe` to be a mutable place. An owned arm binding has
no `mut` spelling (Batch 1 A5: "No extra `mut` after `as` is required"),
and neither does `for value in own values:`.

**Recommend that every binding introduced by a consuming form — a `match
own` payload or type-pattern binding, a `for ... in own` loop target, and
tuple-unpack leaves of those forms — is a mutable place**, like the fresh
managed binding `with` creates ([sema.rs:4318–4324](../crates/aura-compiler/src/sema.rs)).
Such a binding is a fresh owner with no alias; requiring a marker that
cannot be written would be a dead end, and making the owner mutable exposes
nothing that an ordinary `mut` local would not. Ordinary `name = value`
locals keep requiring `mut`, so Python-shaped rebinding control is
unchanged. Bare and `match mut` bindings keep their shared and contained
mutable meanings.

```text
def close_stdin(child: process.Child) -> Result[None, process.Error]:
    match own child.stdin():
        case process.Pipe as pipe:
            try pipe.write_all("hello\n")
            pipe.close()
        case None:
            pass
    return Result.Ok(None)

def drain(jobs: own list[Job]) -> None:
    for job in own jobs:
        job.tries += 1
        finish(job)
```

The alternative adds `case Type as mut name` and `for mut value in own
values` spellings and keeps owned bindings immutable by default. It is more
explicit and it puts `mut` where no other Aura binding form has it (the
grammar has `mut` before a name only in assignment and `view`); the Manual's
existing `close_stdin` example would need the marker. The recommendation
keeps that example valid.

## I. Disposition of the H2 narrowing gaps — Q27

Two pre-existing Batch 1 narrowing gaps were recorded during H2: a
top-level script binding narrowed by `is not None` still refuses the member
call at run time on both backends ([work record:209–213](../work/2026-09-19-batch-1-phase-2.md)),
and value-position short-circuit narrowing (`print(x is not None and x)`)
is refused by the validator on both backends while the statement form works
([work record:221–225](../work/2026-09-19-batch-1-phase-2.md)).

**Recommend assigning script-scope narrowing to Batch 2 phase 2a and
deferring value-position `and` narrowing to Batch 4.** Script-scope
narrowing is a checker/validator asymmetry of the same class as H1 (the
checker accepts what lowering does not apply), it is small (apply the
existing facts to entry-script bindings, which are ordinary mutable locals
of the entry function), and Batch 2 adds view places over elements whose
narrowing must behave identically at script scope and in functions.
Value-position narrowing needs the validator to carry tag proofs through
expression contexts rather than statement edges; it is expression polish
with no loan interaction and belongs with Batch 4's everyday-syntax work.
The alternatives are to take both in Batch 2 (adds an unrelated validator
change to a loan-heavy phase) or to defer both (leaves a script-scope
behavior that Batch 2 fixtures would have to avoid).

## J. Proposed diagnostics — Q28

**Recommend four new codes — `AU2016`, `AU2017`, `AU3011`, `AU3012` — and
reuse of the established families**, following the registry rule that codes
are append-only and phase-banded ([diagnostics.md:19–28, 68–76](../docs/manual/diagnostics.md)).
The Batch 1 family ended at `AU2015`; `AU3010` is the last loan code. Message
templates describe semantics; implementation substitutes canonical types,
places, and names and attaches origin/use spans. No code is added for a
removed or changed spelling: a program that relied on unenforced `close`
receivers receives the ordinary `AU3003`.

| Code | Proposed message template / use |
| --- | --- |
| AU1101 | `expected ',' or ':' after with item`; malformed multi-item `with`; other ordinary syntax errors (no parser change is needed for element, entry, slice, or `lookup` forms) |
| AU2001 | ``unknown member `append` on `Span[int64]` `` with guidance naming the base list; ordinary unknown names |
| AU2002 | Existing type mismatch, including a `Span[T]` argument for a `list[T]` parameter (guidance: `.to_list()`), an initializer argument type, and an undetermined class parameter at an `__init__` class call |
| AU2004 | Existing binding errors, including a field name supplied to an `__init__` class call and a positional argument reaching a keyword-only initializer slot |
| AU2005 | `lookup outcome may only be matched`; `__init__` called as a method or used as a value; `match own` on a lookup outcome |
| AU2006 | Existing builtin-member collision, now covering a user trait `lookup` on `list`/`dict` |
| AU2010 | `Span[T]` written as a union member; existing union member errors |
| AU2013 | Existing union arm coverage (a `Found` payload view's own `None` narrowing uses the union rules) |
| AU2016 | `initializer must be `def __init__(self, ...)` with a bare `self` receiver`; a result annotation other than `None`; initializer type parameters; `return value` in an initializer; a `public` marker conflict is not an error (E3) |
| AU2017 | ``class `M` is not a context manager: `__exit__(mut self)` is missing or has the wrong shape``; ``entry of `M` must return `None` or a view of `self` ``; ``exit of `M` may fail with `E`; the enclosing function must return `Result[_, E]` `` |
| AU3001 | Existing use after move, including an element moved by `pop`/`set` while its owner remains locked |
| AU3002 | Existing loan conflict: shared access to a collection under a mutable element loan, exclusivity between overlapping element loans, an entry loan versus a mutable root access, a move out of a view |
| AU3003 | Existing mutation through an immutable place, now uniformly including `mut self` builtin members on shared views, bare parameters, shared match bindings, and immutable owned locals (H1) |
| AU3004 | Existing invalid place/mode, including `view mut` of an element in an immutable collection and an owned use of `self` inside `__init__` |
| AU3005 / AU3006 | Existing non-Copy indexed read / compound assignment, with guidance updated to name `view`, `.clone()`, `pop`, `set`, `remove`, and `view mut` (ADR-0061's required guidance update) |
| AU3007 / AU3009 | Existing clone-safety and single-consumer duplication, including `Span.to_list()` and value-position span slices |
| AU3008 | Existing Transfer boundary, including a `Span[T]` or element view in a task capture |
| AU3010 | Existing view escape and returned provenance, now including `Span[T]` in an owned position, an entry view escaping its `with` body, and a lookup payload escaping its arm |
| AU3011 | ``mutating `items` with `append` would invalidate the live view `head` ``; labels the view creation, its final use, and the invalidating operation; covers every whole-collection mutation and non-disjoint element write under a live element, entry, or range loan |
| AU3012 | ``field `label` may be uninitialized here``; ``partially initialized `self` cannot escape``; ``initializer exits without initializing `data` ``; ``field `data` may already be initialized; assignment needs a known state``; labels the path and any prior initialization |
| AU4003 | Existing bounds/lookup trap, including element and range loan creation with an invalid position, an absent key, or a reversed range |
| AU4005 | Existing resource failure, including a trap raised by an exit while a body failure is primary |

`AU3011` is distinct from `AU3002` so that tools can separate "this
operation would move or reallocate storage under a view" from "two accesses
overlap"; ADR-0061 asks for the origin and invalidating operation to be
named, which this code carries. `AU3012` is distinct from `AU3001` because
an uninitialized field was never moved and the guidance differs. The
alternative reuses `AU2999`, `AU3002`, and `AU3004` for every new failure;
it keeps the registry smaller and makes an invalidation indistinguishable
from an overlap in the LSP.

## K. Reference agent before and after

The maintained [tool_runner version 1](../examples/agents/tool_runner/src/main.au)
(144 lines, pinned by the byte-identical MIR/direct stdout) is the oracle.
Its `dispatch` ([lines 78–85](../examples/agents/tool_runner/src/main.au))
removes, calls, and reinserts a tool through a `mut` registry because
`dict.get` clones and a packed `Tool` is non-cloneable; the Batch 1
checkpoint recorded this as the limitation Batch 2 must remove
([16-batch-1-design-checkpoint.md:1435–1441](16-batch-1-design-checkpoint.md)).

After Batch 2 (C1 and A6), `dispatch` takes a shared registry and probes
once; `run_tools` no longer needs `mut registry`:

```text
def dispatch(request: ToolRequest, registry: dict[str, Tool]) -> Result[ToolResult, ToolError]:
    match registry.lookup(request.tool):
        case Lookup.Found(tool):
            return tool(value=request.value)
        case Lookup.Missing:
            return Result.Err(ToolError.UnknownTool(request.tool.clone()))
```

After Batch 3 (E1, E3, F1, F5), `Session` gains an initializer that
derives its label and an exit that reports a typed cleanup outcome, and
`main` keeps the same shape:

```text
class Session:
    name: str
    label: str

    def __init__(self, name: own str):
        self.label = f"session:{name}"
        self.name = name

    def __exit__(mut self) -> None:
        print(f"closed {self.name}")

def main() -> int32:
    print("tool-runner version 2")
    with session = Session("tool-runner"):
        match run_tools():
            case Result.Ok(code):
                return code
            case Result.Err(error):
                print(error)
                return 1
```

The expected stdout changes only in the version header; every other line
matches version 1. Each phase publishes the actual diff and pinned output on
both backends, as the Batch 1 phases did. What does not become simpler:
`from_json` still clones nothing but still matches twice; entry views and
slice views have no use in this program, so their usability evidence comes
from the new tutorial examples rather than from the agent; stored loan
callbacks remain unavailable (D).

## L. Prior art

**Python.** Adopt `__init__`, `__enter__`/`__exit__`, `with a, b:`, and the
`if key in table` idiom as spellings. Reject `__exit__`'s exception-suppression
return value (ADR-0060 forbids it), `__enter__` returning an arbitrary
object (Aura's entry is unit or a view of the manager), and dynamic
attribute creation in `__init__` (fields are declared). Python's
`dict.get(key, default)` is the app-facing lookup this checkpoint declines
to imitate for non-clone-safe values; `lookup` is the borrowing form. See the
[data model](https://docs.python.org/3/reference/datamodel.html#with-statement-context-managers)
and [`with` statement](https://docs.python.org/3/reference/compound_stmts.html#the-with-statement).

**Rust.** Adopt the distinction between `&T`/`&mut T` into a `Vec` element
and the borrow of the whole vector (every `push` under a live element borrow
is an error), `&[T]` as a view-only slice type (`Span[T]`), and drop-order
discipline for partially constructed values. Reject lifetime parameters
(Aura infers regions), `Option<&T>` results (a union member may not be a
view), `Drop` as a general destructor hook, and `?` on construction (Aura's
initializer cannot fail with a typed error). See the
[slice type](https://doc.rust-lang.org/std/primitive.slice.html) and
[`Entry` API](https://doc.rust-lang.org/std/collections/hash_map/enum.Entry.html),
whose insert-or-view shape A2 rejects for hidden insertion.

**Swift.** Adopt definite initialization for stored properties before
`self` is used, which is Swift's two-phase initialization rule, and
`defer`-free cleanup ordering through scopes. Reject failable initializers
(`init?`) for Batch 3 (E5 keeps factories) and copy-on-write collection
semantics, which would hide the clone. See
[Initialization](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/initialization/).

**Mojo.** Adopt origin-tracked references into collections and the view-only
`Span` type name and shape. Reject exposing origin parameters in source.
See [Mojo lifetimes and origins](https://docs.modular.com/mojo/manual/values/lifetimes)
and [`Span`](https://docs.modular.com/mojo/stdlib/memory/span/Span/).

## M. Implementation and acceptance plan

### M1. Ordering and shared rules

Four phases, in this order, each merged only after its own complete
verification: **2a** element and entry loans, iteration unification,
`lookup`, the resource capability policy, and script-scope narrowing;
**2b** slice views and `Span[T]`, returned element/entry/range views, the
callable footprint kinds, and the dictionary lookup review record; **3a**
`__init__`, definite initialization, factories, and the exit-action
extension; **3b** context managers. Phase 2a precedes 2b because ranges are
defined over element projections; 3a precedes 3b because `ManagerExit` and
entry views depend on the typed exit-action stack and on initializer-built
managers. The lifetime-storage checkpoint (D) is scheduled after 3b.

Rules shared by every phase, restating the roadmap's sequencing
([roadmap:248–262](14-priority-roadmap.md)):

- Ratification is folded into ADR-0061, ADR-0059, ADR-0060, and dated
  amendments of ADR-0038 (indexed and range places, iteration unification,
  typed exit actions), ADR-0044 (`lookup`, `Span[T]`, the receiver-table
  audit), ADR-0014/ADR-0016 (`AU3005`/`AU3006` guidance), ADR-0005/ADR-0006
  (owned bindings as mutable places), and ADR-0052 (a note that `Span[T]` is
  a view type and therefore not a member) before code changes.
- Failing fixtures first: every check-fail family lands before its
  run-pass family; every run-pass and run-fail fixture runs on MIR and
  forced direct execution with byte-identical stdout; no unsupported case
  hides behind automatic backend fallback.
- Every new MIR contract (element/range loan sources, lookup lowering,
  typed exit actions, initializer slots) has a forged-MIR test that fails at
  `run_mir` and `emit_host_native_object` with one shared reason before
  either backend relies on it, in the pattern of
  [batch1_validator_coverage.rs](../crates/aura-compiler/tests/batch1_validator_coverage.rs).
  Security-sensitive containment tests follow AGENTS.md's standing
  defensive-review delegation during implementation.
- Coverage floors move only upward; production code stays in ordinary
  module files under the coverage regex; LSP coverage stays at 100%; the
  extension adds grammar, tokens, and snippets only.
- One complete local `npm run ci` chain and one green hosted CI run per
  phase; the reference agent's diff and pinned output are the usability
  evidence for 2a/2b (dispatch) and 3a/3b (Session).
- Heavy builds observe the repository's `target/` and disk policy.

### M2. Phase 2a — element and entry loans

Compiler: extend `PlaceProjection` with `Element(selector)` and
`Entry(selector)` in `sema/places.rs`; extend `view_place`
([loans.rs:782–806](../crates/aura-compiler/src/sema/loans.rs)) and
`Stmt::View` checking ([sema.rs:3569–3650](../crates/aura-compiler/src/sema.rs));
add the contextual element read to argument, receiver, operand, and
scrutinee checking; add the A3 classification to every `BuiltinMember`;
convert loop bindings to `ViewBinding` and retire `frozen_places` for lists;
add `lookup` as a compiler-known member with scrutinee-only checking; apply
the audited receiver table in `require_mutable_receiver`; make owned
pattern/loop bindings mutable places; apply narrowing facts to entry-script
bindings.

MIR and validator: `BeginLoan`/`Reborrow` gain a `selector: Option<Operand>`
beside the place string ([mir.rs:741–757](../crates/aura-compiler/src/mir.rs));
the lookup probe becomes `Probe { target, collection, key }` plus a guarded
`BeginLoan`; `MutableIterationWriteback` is removed from list lowering; the
validator proves evaluate-once selectors, presence-dominated lookup loans,
and the A3 invalidation rule over MIR calls and stores.

Backends: the interpreter's loan arena descriptor gains an element/entry
selector resolved against the collection's storage; the direct backend's
`DirectViewPlace` alternatives gain a computed selector value
([native_codegen.rs:4479–4481, 5174–5283](../crates/aura-compiler/src/native_codegen.rs)).
Neither backend may clone an element and write it back.

| Fixture family to add first | Parse/check expectation | Runtime oracle and parity |
| --- | --- | --- |
| `element_view_*`, `entry_view_*` | Check-pass shared/mutable element and entry views of locals, fields, parameters, existing views, nested projections; check-fail immutable collection `AU3004`, move out of view `AU3002`, escape `AU3010` | Reads see immediate writes; negative index normalized once; rebinding the index does not retarget; both backends print pinned values |
| `element_contextual_read_*` | `print(tags[0])`, `tags[0].clone()`, `tags[0] == other`, f-string, bare-match scrutinee, `mut` receiver on an element; check-fail owned read `AU3005` with the new guidance | No clone counter movement on shared reads; one clone on `.clone()` |
| `element_invalidation_*` | Every whole-collection operation and non-disjoint indexed assignment under a live element/entry/range loan reports `AU3011`; literal-disjoint element writes pass; dictionary indexed assignment under any entry loan fails | Passing programs run identically on both backends; no runtime invalidation path exists |
| `element_bounds_*` | Check-pass | `AU4003` at loan creation for out-of-range, absent key, and reversed range on both backends; nothing registered before the trap |
| `iteration_element_loan_*` | Reborrow from a loop binding; other-position shared views under bare iteration; check-fail under mutable iteration `AU3002` | `list_mut_iteration` stdout unchanged; a trap after a write in a `mut` loop body retains the write (run-fail with pinned partial output) |
| `lookup_arm_*` | Bare and `mut` `lookup` on list and dict, guards, `Found` payload narrowing with an optional `V`; check-fail bound/stored/returned outcome `AU2005`, `match own` `AU2005`, payload escape `AU3010` | One probe per match (probe counter); present `None` prints `found none`; `Missing` prints `missing`; mutable writes visible after the match |
| `resource_capability_*` | `mut self` builtin members on shared views, bare parameters, shared match bindings, and immutable owned locals report `AU3003`; `match own` arm bindings and `own` loop targets accept mutation; Queue `put`/`close` on bare parameters pass | `close_stdin` example runs; the reference agent's `events.close()` runs unchanged; a table-versus-Manual characterization test |
| `script_scope_narrowing_*` | Check-pass | Top-level `if found is not None: print(found.len())` prints on both backends |
| `element_loan_validator_*` (Rust) | Forged MIR: selector re-evaluation, loan not dominated by the probe, structural mutation under a live element loan, cross-task element descriptor, lookup loan escaping its arm | Same refusal reason at `run_mir` and `emit_host_native_object` |

Manual chapters: Ownership and Borrowing, Collections, Statements,
Execution Model, Enums and Match, Static Semantics, Diagnostics, Current
Limits, API Index, Process, Filesystem, Network, Concurrency (receiver
table). Examples, tutorials, generated documents, and editor grammar follow
in the same change family.

### M3. Phase 2b — slice views and returned collection views

Compiler: `Span[T]` as a builtin view-only type in `sema/types.rs` with
its operation table; `PlaceProjection::Range(start, end)`; `view` over a
slice expression; contextual range access at `Span[T]` parameters and
whole-list coercion; the `Element`/`Entry`/`Range` footprint kinds in
returned-view metadata, checked interfaces (schema bump), stored callable
contracts, and hover. MIR: range selectors on `BeginLoan`/`Reborrow`;
`BeginReturnedLoan` projections gain selector alternatives. Backends: a
span descriptor (base, offset, length) touched only by span operations.

| Fixture family to add first | Parse/check expectation | Runtime oracle and parity |
| --- | --- | --- |
| `span_view_*` | Shared/mutable spans, nested reborrows, whole-list coercion to `Span[T]` parameters, value-position slices of spans; check-fail owned `Span[T]` positions `AU3010`, union member `AU2010`, structural member `AU2001`, task capture `AU3008` | Pinned output for element writes through spans, `sort`/`reverse`/`swap` on a range, `to_list` clone counting |
| `span_bounds_*` | Check-pass | `AU4003` for reversed or out-of-range endpoints at creation and for nested ranges |
| `returned_collection_view_*` | Returned element, entry, and range views from functions, methods, trait methods, imported modules, and stored callables; check-fail different roots `AU3010`, task target `AU3008`, `from self` packing `AU3010` | Caller lock covers the whole origin; the exact element is written through; an absent key traps inside the callee before handoff |
| `dict_lookup_review_*` | Check-pass programs demonstrating `lookup`, `get`, and `key in` on optional `V`; no `get_optional` member exists (`AU2001`) | Pinned present-None versus missing output |
| `span_validator_*` (Rust) | Forged range selectors, overlapping mutable ranges, a range outliving its base, a returned range with a wrong origin | Same refusal at both entry points |

Manual chapters: Collections (new `Span[T]` section), Ownership and
Borrowing, Closures (stored view contracts), Functions, Generics and
Traits, Types, Grammar, Static Semantics, Diagnostics, Current Limits, API
Index.

### M4. Phase 3a — initialization and the cleanup mechanism

Compiler: `__init__` recognition in class checking; initializer-authority
`self` in the checker's receiver handling; a per-field initialization state
in `sema/flow.rs` using the existing join and loop machinery; class-call
binding against the initializer contract; visibility rule; `AU2016`/`AU3012`.
MIR: `PushExitAction`/`PopExitAction` replace `PushCleanup`/`PopCleanup`
with typed actions; class calls with initializers lower to slot allocation
plus a call passing the slot; field initialization pushes `DropInitialized`.
Backends: the interpreter represents an uninitialized field slot explicitly
and its `cleanup_stack` becomes typed; the direct backend allocates class
storage before the call and gains a drop-field thunk kind.

| Fixture family to add first | Parse/check expectation | Runtime oracle and parity |
| --- | --- | --- |
| `init_declaration_*` | Bare `self`, defaults, keyword-only, generic classes, `copy class`; check-fail `mut self`/`own self` `AU2016`, result annotation `AU2016`, method call on `__init__` `AU2005`, field-constructor arguments `AU2004`, own use of `self` `AU3004` | Evaluation order trace: supplied args, parameter defaults, field defaults, body; hover/signature help show the initializer contract |
| `init_definite_*` | Branches, loops, nested fields, helper calls on initialized projections, `own` reinitialization; check-fail read-before-init, `MaybeInit` assignment, incomplete escape, incomplete return, each `AU3012` with labelled paths | Pinned outputs for computed fields on every branch |
| `init_visibility_*` | Private `__init__` blocks the cross-module class call (`AU2001`/`AU2004` per the existing visibility codes); `public def __init__` admits it; factories construct privately | Imported construction prints pinned values |
| `init_partial_cleanup_*` | Check-pass | A trap after two of three fields are initialized drops exactly those two, in reverse order (drop counters); cancellation inside `__init__` at a scheduler-aware wait drains the same actions; completion drops nothing twice |
| `factory_*` | `Result`-returning associated functions with explicit failure-path close; check-fail `__init__ -> Result` `AU2016`; a `with` binding moved into a constructor `AU2999` as today | Builtin resource locals dropped on a failing `try` release their host handles (verified, not assumed); typed errors preserved |
| `exit_action_validator_*` (Rust) | Forged MIR: `run: false` pop on an incomplete path, pop of a non-top action, `DropInitialized` outliving the frame, push not dominated by the initializing store | Same refusal at both entry points |

Manual chapters: Classes (construction rewritten), Ownership and Borrowing,
Functions, Execution Model, Static Semantics, Grammar, Diagnostics, Current
Limits, Names and Scopes, API Index.

### M5. Phase 3b — context managers

Compiler: manager shape detection replacing `require_with_resource`
([sema.rs:15095–15135](../crates/aura-compiler/src/sema.rs)); multi-item
`with` grammar; entry shape checking; hidden managed bindings for entry
views; `AU2017` static exit-result check; generic managers. MIR: `ManagerExit`
actions with an exit-result kind; entry-view lowering as a returned view
from the managed place; exit `Err` propagation on normal paths and
discard/attach on failing paths. Backends: the interpreter and direct
runtime run `__exit__` through the typed action and route its `Result`.

| Fixture family to add first | Parse/check expectation | Runtime oracle and parity |
| --- | --- | --- |
| `manager_protocol_*` | Dunder managers, `close`-only managers, generic managers, both binding forms; check-fail wrong shapes `AU2017`, owned/`Result` entry `AU2017`, non-manager `AU2017` | Entry/exit order trace on every exit path: fallthrough, `return`, `break`, `continue`, `try`, trap, cancellation |
| `manager_multi_item_*` | Multi-item headers, later items using earlier bindings; parse-fail malformed lists `AU1101` | Failing second entry (typed, trap, cancellation) exits the first; reverse exit order pinned |
| `manager_entry_view_*` | Shared and mutable entry views from named and hidden managers; check-fail entry view escape `AU3010`, moving/closing the named manager under a live entry view `AU3002` | Writes through the entry view are visible after exit; exit runs after the view's last use |
| `manager_exit_result_*` | `Result`-returning exits in `Result` functions; check-fail in a non-`Result` function `AU2017` | Normal-path exit `Err` becomes the function result and discards a computed `return` value; a body `Err` wins over an exit `Err`; a trap attaches an exit `Err` as related information |
| `manager_cancellation_*` | Check-pass | Cancellation observed in the body runs `__exit__` with cancellation pending; a scheduler-aware wait inside the exit returns `Cancelled`; scheduler abandonment after a forced reset runs no exit (pinned through the existing multicore isolation harness) |
| `manager_validator_*` (Rust) | Forged `ManagerExit` without a preceding entry, exit result kind mismatch, entry view descriptor outliving the managed place | Same refusal at both entry points |

Manual chapters: Statements (`with` rewritten), Classes, Execution Model
(resource lifetime and the abandonment boundary), Ownership and Borrowing,
Concurrency, Grammar, Static Semantics, Diagnostics, Current Limits,
Filesystem, Process, Network.

### M6. Relative size estimates

Planning ranges in fixture **files**, as in the Batch 1 checkpoint; unit,
interface, validator, and LSP tests are additional. The calibration points
are H1 (192–269 estimated fixture files, about 20–30 Rust modules,
[16-batch-1-design-checkpoint.md:1600–1612](16-batch-1-design-checkpoint.md))
and H2 (61 conversions plus the replacement families across roughly 300
files, with local `npm run ci` chains of about 80 minutes,
[work record:149–166](../work/2026-09-19-batch-1-phase-2.md)).

| Phase | Parse-pass | Check-pass | Check-fail | Run-pass | Run-fail | Touched modules/surfaces | Relative size |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| 2a element/entry loans, iteration, `lookup`, capability policy, script narrowing | 2–4 | 22–30 | 40–55 | 35–48 | 8–12 | `sema/{places,loans,capabilities,flow,builtins,patterns}.rs`, `call.rs`, `mir.rs` (lowering and validator), `mir_runtime.rs`, `native_codegen.rs`, `native_runtime.rs`, `analysis.rs`; about 12 Manual chapters; every fixture/tutorial/example calling a `mut` builtin member on an immutable local | Comparable to H1's loan work in compiler depth, about half its surface; chains comparable to H2 because both runtimes change |
| 2b slice views and returned collection views | 4–6 | 14–20 | 26–36 | 22–30 | 6–9 | `sema/types.rs`, `sema/loans.rs`, `sema/callables.rs`, interface schema, `mir.rs`, both runtimes, LSP hover; about 11 Manual chapters | About half of 2a |
| 3a initialization and exit actions | 4–6 | 18–26 | 34–46 | 26–36 | 8–12 | `sema/{program,flow,capabilities}.rs`, class checking, `mir.rs` (class calls, exit actions, validator), both runtimes, LSP signature help; about 10 Manual chapters | Comparable to 2a in compiler depth; smaller runtime surface |
| 3b context managers | 4–6 | 16–22 | 28–38 | 24–34 | 8–12 | `sema.rs` `with` checking, parser, `mir.rs`, both runtimes, cancellation harness; about 11 Manual chapters | About two thirds of 3a |

Totals: about 107–145 new fixture files for Batch 2 and 116–162 for Batch
3, plus existing-fixture updates driven by H1 (estimated 20–40 files whose
programs call a `mut` builtin member through an immutable place). Backend
parity covers every new run-pass/run-fail fixture. If an inventory grows,
finish the surface rather than treating the estimate as a stopping
condition.

## Reconciliation and confidence

| Records pulling in different directions | Proposed resolution |
| --- | --- |
| ADR-0038's rejection of indexed places versus Approved Decision 5 and ADR-0061 | A1–A3 supply the deferred items ADR-0038 names (evaluate-once selection, generation/reallocation rules, bounds, overlap, invalidation, parity); the amendment is dated and the rejection text becomes history |
| ADR-0040's owned slices versus ADR-0061's slice views | B1 keeps value-position slices owned and reserves the `view` introducer for range loans; no reinterpretation of existing syntax |
| Q23 A's retained `Lookup[V]` versus the required app-facing `V \| None` review | C1 adds the borrowing `lookup` form; C2 records that a `V \| None` result cannot carry a loan and would flatten present `None`, so none is added; the tagged owned `get` stays |
| ADR-0052's view-member exclusion versus entry views into optional values | The `Found` payload is a view of the stored union; presence is the arm and `None` is a narrowing test on the view (C1) |
| ADR-0058/ADR-0061's one joint lifetime design versus two batches that need only pieces of it | D fixes the footprint vocabulary now and names a lifetime-storage checkpoint after Batch 3 as the owner of stored loans |
| ADR-0059's initializer-specific `self` authority versus the shared meaning of bare `self` | E1 gives `self` an initializing-place authority that transitions to `mut self` behavior at completion; ordinary methods are untouched |
| ADR-0059's factory-first fallibility versus the convenience of `__init__ -> Result` | E5 keeps factories and states the reason; the alternative is listed for a later revisit |
| ADR-0060's "entry may fail" versus the impossibility of a view-bearing `Result` | F2 places typed acquisition failure in the manager's factory and keeps trap/cancellation entry failure covered by F3's registration order |
| ADR-0060's body-primary rule versus typed exit failures | F5 propagates exit `Err` only on normal paths, discards it under a body `Err`, and attaches it under traps |
| ADR-0060's cleanup-under-cancellation promise versus ADR-0038's forced-reset boundary | G2 scopes the promise to cooperative cancellation on a live frame and the boundary to scheduler abandonment; neither record is weakened |
| ADR-0038's temporary-origin exclusion versus ADR-0060's header entry view | F4 makes every `with` item's manager a managed place (named or hidden) and uses that place as the origin |
| The builtin receiver table versus the Manual's documented capabilities and the reference agent's `Queue.close()` on a bare parameter | H1 audits the table (Copy handle operations are shared) and then enforces it uniformly |
| Uniform enforcement versus the `match own ... pipe.close()` example | H2 makes owned pattern and loop bindings mutable places |
| ADR-0038's `MutableIterationWriteback` versus write-through element loans | A5 retires the list writeback action; set, Queue, and Range iteration are unchanged |
| The representation phase's interim layouts versus element loans | Scope states the three assumptions element loans may make and the layouts they may not |

The least certain recommendations are **Q9's arm-scoped `lookup`**, because
a scrutinee-only outcome is a new second-class category and the
presence-guard alternative needs no new operation; **Q5's iteration
unification**, because it changes the trap-time semantics of `mut`
iteration and retires an exit action, and the separate-provenance
alternative delivers the same source surface; and **Q21's typed exit
results**, because discarding an exit `Err` under a body `Err` is a policy
choice that a later error-aggregation design might revisit. Each is a real
choice with a workable alternative recorded in the questionnaire.

The questionnaire covers each design bundle: A1–A6 map to Q1–Q6, B1–B2 to
Q7–Q8, C1–C2 to Q9–Q10, D to Q11, E1–E5 to Q12–Q16, F1–F6 to Q17–Q22,
G1–G2 to Q23–Q24, H1–H2 to Q25–Q26, I to Q27, and J to Q28. Section M
contains engineering choices only. Phase ordering, failing-tests-first,
both-backend parity, validator pinning, raise-only coverage floors, and the
clean-slate policy are required constraints rather than optional answers.

## Ratification questionnaire

Each question lists its alternatives and one recommendation. The
**Ratified** line is for the owner's answer.

### Element and entry loans — Q1–Q6

**Q1 — A1: How are element and entry loans spelled and how does contextual access work?**

- A. Extend the existing `view`/`view mut` statement and contextual shared/mutable access to `items[i]` and `table[key]`; owned positions keep `AU3005` with updated guidance; no new introducer.
- B. Keep index expressions out of the place grammar and add builtin returned-view methods only (`items.at(i)`, `items.at_mut(i)`, `table.at(key)`); `print(tags[0])` stays rejected for non-Copy elements.
- **Recommended: A.**
- **Ratified: A.**

**Q2 — A2: How are selectors evaluated and what happens on an invalid position or absent key?**

- A. Evaluate the index or key once at loan creation with the direct-index normalization and `AU4003` trap; presence is tested with `key in` or handled by `lookup`.
- B. Make `view e = table[key]` produce a lookup-shaped view-bearing result that must be matched.
- C. Insert a default and view it when the key is absent.
- **Recommended: A.**
- **Ratified: A.**

**Q3 — A3: Which operations invalidate which loans?**

- A. Classify list indexed assignment as an element-level write with literal-only disjointness, and every other mutating operation (including dictionary indexed assignment) as whole-collection; apply the existing overlap rules to element, entry, and range projections; report `AU3011`.
- B. Treat every mutation, including list indexed assignment, as whole-collection and prove no disjointness.
- **Recommended: A.**
- **Ratified: A.**

**Q4 — A4: How does ownership move through element views?**

- A. No move out of a view; explicit `.clone()`; `pop`, `set`, and `remove` transfer; `set(i, None)` takes an optional element; no new operation.
- B. Also add `items.take(index) -> T` with a required replacement or optional element type.
- **Recommended: A.**
- **Ratified: A.**

**Q5 — A5: Should list iteration bindings become element loans?**

- A. Unify: bare and `mut` list iteration hold a root loan and bind element loans with immediate write-through; `MutableIterationWriteback` is retired for lists; reborrows from loop bindings are allowed.
- B. Keep the separate loop provenance and edge-time writeback; loop bindings are not views.
- **Recommended: A.**
- **Ratified: A.**

**Q6 — A6: May a returned view select an element, entry, or range?**

- A. Yes, with the whole origin root as the static footprint and the exact selector in the runtime token; stored callables admit the same under Q22's single-origin rule.
- B. Defer returned collection views; element loans are local only.
- **Recommended: A.**
- **Ratified: A.**

### Slice views — Q7–Q8

**Q7 — B1: How are slice views spelled and typed?**

- A. `view name = items[a:b]` selects a range projection; the pointee is the view-only builtin `Span[T]`; value-position slices stay owned copies; a whole list supplies a `Span[T]` parameter by reborrow.
- B. `view` over a slice with pointee `list[T]` and a checker-tracked range restriction; every shared list operation accepts sub-ranges on both backends.
- C. As A with the spelling `Slice[T]`.
- **Recommended: A.**
- **Ratified: A.**

**Q8 — B2: What operations do spans support, and are `str` spans included?**

- A. A closed set: length, element access and loans, iteration, nested range reborrows, value-position copies, `to_list`, length-preserving mutators, equality searches; `str` and `Array` spans deferred.
- B. The same set plus `str` spans over byte ranges now.
- **Recommended: A.**
- **Ratified: A.**

### App-facing dictionary lookup — Q9–Q10

**Q9 — C1: What is the app-facing borrowing lookup?**

- A. Arm-scoped `list.lookup(index)`/`dict.lookup(key)` outcomes matched with `Lookup.Found`/`Lookup.Missing` under bare `match` (shared entry loan) or `match mut` (write-through entry loan); not a first-class value.
- B. No new operation; applications use `key in table` followed by an element loan or the owned `get`.
- **Recommended: A.**
- **Ratified: A.**

**Q10 — C2: Should an app-facing `V | None` dictionary lookup be added?**

- A. No: a union member cannot be a view, an owned `V | None` must clone and flattens present `None`; `dict.get -> Lookup[V]` stays and `lookup` is the borrowing form.
- B. Add `dict.get_optional(key) -> V | None`, cloned and owned, rejected when `V` is itself optional.
- **Recommended: A.**
- **Ratified: A.**

### Lifetime-bearing callables — Q11

**Q11 — D: What does Batch 2 deliver of the joint lifetime-bearing design?**

- A. Fix the origin footprint vocabulary (`Element`, `Entry`, `Range`) in interfaces, MIR, and stored contracts now; stored loan environments, captured-self origins, and aggregate lifetime propagation are owned by a lifetime-storage checkpoint after Batch 3.
- B. Design and deliver lifetime-bearing callable storage in Batch 2.
- C. Keep owned-only callable storage permanently.
- **Recommended: A.**
- **Ratified: A.**

### Initialization — Q12–Q16

**Q12 — E1: How is `__init__` declared and what is `self` inside it?**

- A. `def __init__(self, ...)` with bare `self` as an initializing place that becomes `mut self`-like at completion; ordinary parameters; no result annotation; never callable as a method; the object is built in the call's result slot.
- B. `def __init__(mut self, ...)` treated as an ordinary mutable method over a pre-allocated slot.
- **Recommended: A.**
- **Ratified: A.**

**Q13 — E2: How do field defaults and the field-based constructor interact with `__init__`?**

- A. Declaration defaults are evaluated before the body and are definitely initialized at entry; a class with `__init__` has no field-based class call.
- B. No field defaults are applied when `__init__` exists; the body must initialize every field.
- **Recommended: A.**
- **Ratified: A.**

**Q14 — E3: How does `__init__` visibility work across modules?**

- A. `__init__` follows ordinary method visibility; a private initializer makes the class call module-local and external code uses public factories; `public def __init__` opens the class call.
- B. Every `__init__` is callable wherever its class is visible; a visibility marker on it is ignored.
- **Recommended: A.**
- **Ratified: A.**

**Q15 — E4: How is definite initialization checked?**

- A. Flow-sensitive per-field `Uninit`/`Init`/`MaybeInit` state with the existing join and loop rules; initialized projections may be read, lent, and passed; `self` as a whole requires completion; `AU3012`.
- B. A linear-prefix rule: every field is assigned by straight-line statements before any branch, loop, call, or read of `self`.
- **Recommended: A.**
- **Ratified: A.**

**Q16 — E5: How does fallible construction work?**

- A. Named associated factories returning `Result[Class, Error]`; `__init__` cannot return `Result`; partial state is cleaned by ordinary owned-local drops and G1; no release of a managed `with` binding.
- B. Also admit `def __init__(self, ...) -> Result[None, E]`, making the class call a `Result[Class, E]` expression.
- **Recommended: A.**
- **Ratified: A.**

### Context managers — Q17–Q22

**Q17 — F1: What is the manager protocol?**

- A. Shape-matched `__enter__(mut self)`/`__exit__(mut self)` dunders, with `close(mut self) -> None` retained as the implicit manager; no trait.
- B. A builtin nominal `ContextManager` trait with `enter`/`exit` methods and an associated entry type, implemented with `impl`.
- **Recommended: A.**
- **Ratified: A.**

**Q18 — F2: What may `__enter__` return?**

- A. Nothing (unit) or `view [mut] T from self`; typed acquisition failure belongs to the factory expression in the header (`with tx = try conn.begin():`).
- B. Also `Result[None, E]` with an implicit `try` at the `with` header.
- **Recommended: A.**
- **Ratified: A.**

**Q19 — F3: Are generic managers and multiple header items admitted?**

- A. Yes: generic manager classes, and `with item, item, ...:` entered left to right with each exit registered after its own entry and run in reverse.
- B. Generic managers only; multiple resources use nested `with`.
- **Recommended: A.**
- **Ratified: A.**

**Q20 — F4: How does a header-constructed manager expose an entry view?**

- A. Every `with` item's manager occupies a managed place — the named binding, or a hidden managed binding when `as name` binds an entry view — and that place is the view's origin.
- B. Entry views are available only from a named manager binding through an explicit `view name = manager.__enter__()` in the body.
- **Recommended: A.**
- **Ratified: A.**

**Q21 — F5: What may `__exit__` return and how do failures compose?**

- A. `None` or `Result[None, E]`; an exit `Err` propagates from the `with` statement on a normal exit path (checked like `try`), is discarded under a propagating body `Err`, and is attached as related information under a trap or cancellation.
- B. `None` only; every cleanup failure is a trap.
- **Recommended: A.**
- **Ratified: A.**

**Q22 — F6: How does cancellation interact with exits?**

- A. Exits run with cancellation pending and no shield; scheduler-aware waits inside an exit observe cancellation immediately.
- B. Each exit runs under a fixed grace period during which cancellation is masked.
- **Recommended: A.**
- **Ratified: A.**

### Cleanup mechanism — Q23–Q24

**Q23 — G1: What is the one partial-construction cleanup mechanism?**

- A. Typed exit actions on the existing ordered stack with two new kinds, `DropInitialized` and `ManagerExit`, statically disarmed at completion and proven by the validator; Batch 6 decoders reuse `DropInitialized`.
- B. A runtime-side construction scope object that tracks initialized fields dynamically.
- **Recommended: A.**
- **Ratified: A.**

**Q24 — G2: How are cleanup under cancellation and forced frame reset reconciled?**

- A. The cleanup promise covers cooperative cancellation on a live frame; scheduler abandonment after a forced reset releases only runtime-owned state and runs no Aura exit; the Manual states the boundary.
- B. Snapshot managers into runtime cells so that `__exit__` can run on the host side after a reset.
- C. Pin every frame holding a registration so that it is never reset.
- **Recommended: A.**
- **Ratified: A.**

### Resource capability policy — Q25–Q26

**Q25 — H1: How are builtin receiver capabilities enforced?**

- A. Audit the table (operations on Copy runtime handles such as `Queue` are shared; value and non-Copy resource mutators stay `mut`) and enforce it uniformly at every call site with `AU3003`, agreeing with the validator.
- B. Keep enforcement at explicit sites and make the validator accept what the checker accepts.
- C. Reclassify every builtin `close` as a shared interior-state operation.
- **Recommended: A.**
- **Ratified: A.**

**Q26 — H2: Are owned pattern and loop bindings mutable places?**

- A. Yes: `match own` bindings, `for ... in own` targets, and their unpack leaves are mutable places, like the managed `with` binding; ordinary locals still need `mut`.
- B. No: add `case Type as mut name` and `for mut value in own values` spellings.
- **Recommended: A.**
- **Ratified: A.**

### Deferred narrowing items — Q27

**Q27 — I: Where do the two H2 narrowing gaps land?**

- A. Script-scope narrowing in Batch 2 phase 2a; value-position `and` narrowing in Batch 4.
- B. Both in Batch 2 phase 2a.
- C. Both deferred to Batch 4.
- **Recommended: A.**
- **Ratified: A.**

### Diagnostics — Q28

**Q28 — J: Which diagnostic policy accompanies the design?**

- A. Add `AU2016` (initializer contract), `AU2017` (manager protocol and exit result), `AU3011` (invalidating mutation under a live view), and `AU3012` (definite initialization); reuse the existing families, with `AU3005`/`AU3006` guidance updated.
- B. Reuse `AU2999`, `AU3002`, and `AU3004` for every new failure.
- **Recommended: A.**
- **Ratified: A.**
