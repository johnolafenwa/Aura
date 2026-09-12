# ADR-0058: First-class callables and binding contracts

- Status: Accepted; detailed design ratified 2026-09-08; implementation in Batch 1
- Date: 2026-09-06
- Implementation: Phase 1 in progress; Option removal remains phase 2
- Roadmap: Batch 1
- Extends: ADR-0013, ADR-0037, ADR-0038, and ADR-0051
- Required by: ADR-0053

## Ratified detailed design

Ratified on 2026-09-08; recorded in design commit `7685199`. The
[design checkpoint](../16-batch-1-design-checkpoint.md) is normative where a
retained baseline below differs. The answers are:

- Q13: A — thin def remains Copy; explicit owned Callable types add storage ([design section](../16-batch-1-design-checkpoint.md#callable-rules--q13q22)).
- Q14: A — derive call kind from owned use; allow explicit one-way restrictions ([design section](../16-batch-1-design-checkpoint.md#c2-call-kinds-capture-ownership-and-local-mutation--q14)).
- Q15: A — three-word inline environment and checked overflow allocation ([design section](../16-batch-1-design-checkpoint.md#c3-inline-versus-allocated-environments--q15)).
- Q16: A — shared operations table supplies indirect destruction ([design section](../16-batch-1-design-checkpoint.md#c4-destruction-and-unavoidable-erased-dispatch--q16)).
- Q17: A — explicit common contracts and invariant safe restrictions ([design section](../16-batch-1-design-checkpoint.md#c5-heterogeneous-values-names-restrictions-and-variance--q17)).
- Q18: A — bound methods move owned receivers or snapshot Copy receivers ([design section](../16-batch-1-design-checkpoint.md#c6-bound-methods-on-owned-or-copy-receivers--q18)).
- Q19: A — names, modes, keyword-only boundary and default availability form identity ([design section](../16-batch-1-design-checkpoint.md#c7-defaults-keyword-only-declarations-and-forwarding--q19-and-q20)).
- Q20: B — ordinary wrappers declare their own policy; no wrap intrinsic ([design section](../16-batch-1-design-checkpoint.md#c7-defaults-keyword-only-declarations-and-forwarding--q19-and-q20)).
- Q21: A — TaskCallable admission preserves structural Transfer evidence ([design section](../16-batch-1-design-checkpoint.md#c8-structural-transfer-and-task-targets--q21)).
- Q22: A — stored result views may originate only in one explicit ordinary argument ([design section](../16-batch-1-design-checkpoint.md#callable-rules--q13q22)).
- Q24: A — AU2010–AU2015 plus the existing diagnostic families ([design section](../16-batch-1-design-checkpoint.md#library-and-diagnostic-rules--q23q24)).

Phase 1 implements the type and owned-callable foundations. Existing Option
library signatures, `T?`, and `*_or_none` names remain until phase 2. Stored
loan captures and captured-self result origins remain outside phase 1.

## Authority and current boundary

The user approved the [roadmap](../14-priority-roadmap.md), including the
follow-up explanation of stored closures, bound methods, calling capabilities,
and keyword-only restrictions. This records that future contract. Current
written `def(...)` types describe capture-free code pointers; implemented
compiler-known closure sites and ADR-0038 loan captures remain the baseline.

## Accepted decisions

Functions, capturing closures, and bound methods can be passed as arguments,
returned, and stored in fields or collections through suitable callable types.
The callable representation preserves its environment and these call kinds:

| Kind | Required access | Reuse |
| --- | --- | --- |
| Shared | Shared access to callable and captured state | Repeated calls |
| Mutable | Exclusive mutable access during each call | Repeated calls |
| Consuming | Ownership of callable/captured value consumed by invocation | One call |

Call kind is distinct from each argument's bare, `mut`, or `own` capability.
A repeatable callback may consume a fresh argument on every invocation.
Calling does not implicitly clone captures or consumed inputs.

Owned captures remain alive with their callable after the creating function
returns. Loan captures retain their origin/lifetime constraints; storage does
not let a callback outlive a borrowed owner or hide mutable aliasing. Support
for general storage must include a sound lifetime-bearing contract, rather
than erasing loan provenance at a type boundary. Aggregates containing loan
callbacks inherit the necessary lifetime constraints.

A bound method retains its receiver relation. Shared methods retain shared
access, mutable methods require exclusive access, and an owning receiver moves
into a consuming callback. The receiver cannot be destroyed or invalidated
while a retained loan requires it. Q18 settles owned/Copy receiver acquisition; retained loan storage and
reborrowing remain the joint Batch 1–2 design.

Cross-task use requires the full environment to satisfy structural `Transfer`
and the task API's call contract. Shared/mutable loan captures do not become
transferable merely because they are stored in a callable. Capture-free or
owned transferable environments may qualify.

Callable contracts preserve exposed argument names, default availability,
keyword-only restrictions, parameter capabilities, and result/view-origin
contracts. Assignment to a variable cannot silently make a keyword-only
parameter positionally callable. A future intentionally restricted interface
must specify its conversions explicitly; there is no implicit loss of a
declared calling restriction.

## Delivery boundary and detailed design

### Open conflicts

**Joint Batch 1–2, ADR-0038 / ADR-0061:** the lifetime-bearing callable type is
one design shared by callable storage and collection loans. Batch 1 storage
is scoped to ADR-0037 owned/by-value captures and bound methods on owned or
Copy receivers. Storage of ADR-0038 shared/mutable loan captures is deferred
to that joint design. The broad accepted storage behavior above is retained
as the eventual contract, with this explicit delivery split.

Batch 1 also structurally splits `sema.rs` into type representation,
capability checking, and place/loan analysis, since unions change `Type` and
callables change capability checking. It has one checkpoint after unions,
aliases, and owned-capture closures, before the ADR-0052 optional removal.

### Detailed contract

- Source syntax for call kinds, owned environments, and lifetime-bearing
  callable interfaces; inference and callable aliases.
- Type equality, conversions/variance, generic callable bounds, and heterogeneous
  closure storage with a common call contract.
- Default-expression identity, evaluation timing, and forwarding through
  wrappers; names and keyword-only metadata across imports.
- Receiver binding/reborrowing, return origins, and trait-method values.
- Inline versus allocated environments, allocation failure, destruction,
  dispatch/ABI layout, and optimization. No mandatory boxing or garbage
  collection is implied by general callable storage.

Q13–Q22 above settle the owned-capture scope of these details. Lifetime-bearing
loan storage remains deferred. ADR-0051's implemented deferral is resolved by
Q19 for Batch 1, with tests still required before claiming implementation. Decorators use this model rather than a private storage
exception.

## Completion evidence required

Test shared, mutable, and consuming closures through parameters, returns,
fields, collections, and imports; owned-capture survival; invalid loan escape;
bound receiver cleanup and exclusivity; repeat/single-use enforcement; task
Transfer; keyword-only/default/name preservation; and wrapper forwarding.
Both backends and serialized interfaces must preserve the same environment,
capabilities, lifetime contract, and diagnostics. Update formatter, hover,
completion, examples, and the Manual with implementation.
