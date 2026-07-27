# ADR-0022: Implicit shared, `mut`, and `own` capability syntax

- Status: Accepted
- Date: 2026-07-23
- Ratified: 2026-07-27, in Batch 3
- Amends: ADR-0005, ADR-0006, ADR-0013, ADR-0016, and ADR-0017
- Supersedes the syntax of: ADR-0009

## Ratification

All ten ratification questions were answered when this ADR was accepted. The
answers are binding and are recorded with the questions at the end of this
document. In summary:

1. Universal logical sharing, including declaration-known copy types. The ABI
   may pass copied bits; the source-level rules apply uniformly.
2. Bare `match` is shared, `match mut` is mutable, `match own` consumes.
3. Mutable-match writeback occurs on every exit path: normal arm exit,
   `return`, `break`, `continue`, and error propagation.
4. One atomic pre-1.0 flip, protected by a syntax-aware migrator.
5. `borrow` stays a reserved retired keyword for one compatibility window,
   parsed only far enough to emit exact replacement diagnostics.
6. This ADR supersedes ADR-0009's syntax. Copy-valued borrowed returns become
   ordinary owned returns and labels disappear; the containment semantics
   survive as ordinary move rules.
7. The keyword is removed; the word is not banned. Normative vocabulary is
   shared access / mutable access / ownership transfer.
8. Bare is the one canonical shared spelling. No `shared T` alias.
9. One atomic migration across grammar, sema, MIR, direct, LSP, extension,
   fixtures, examples, tutorials, and the Manual, with the cache schema bumped
   so no pre-migration artifact survives.
10. ADR-0013 is amended, not superseded.

Additional ruling: range iteration rejects `mut` and `own` with a teaching
diagnostic rather than accepting them as no-ops.

## Summary

Aurora should consider removing `borrow` from its source-language vocabulary
and expressing callable access with three spellings:

```aurora
value: T       # shared access
value: mut T   # exclusive mutable access
value: own T   # ownership transfer
```

The proposal removes the `borrow` keyword, not the ownership rules that make
shared and exclusive access safe. The compiler would still track shared access,
exclusive mutable access, moves, temporary lifetimes, and declaration-stable
calling conventions internally.

This decision is ratified and implemented. The compiler, language reference,
and the amended ownership ADRs listed above are the authoritative expression
of the resulting language surface.

## Motivation

The common Aurora operation on a non-copy value is to read it without consuming
it. Requiring a keyword for that common operation adds visual noise, while the
capabilities that can change or invalidate caller state deserve explicit
syntax.

The proposed surface makes that hierarchy visible:

- bare means logical shared access for every type; an implementation may pass
  copy bits by value, but that ABI choice does not weaken the source-level
  shared-loan and sequencing rules
- `mut` means the callee may write through the caller's place
- `own` means the callee receives and may consume the value

This preserves explicit ownership boundaries while making ordinary
Python-shaped code easier to read.

## Core callable syntax

### Parameters

```aurora
def inspect(path: Path):
    print(path.filepath)

def rename(path: mut Path, name: String):
    path.filepath = name

def archive(path: own Path):
    store(path)
```

For an ordinary synchronous call, the intended semantics are:

| Source form | Source-level capability | Caller state after return |
| --- | --- | --- |
| `value: T` | shared read-only access | value remains usable |
| `value: mut T` | exclusive mutable access | value remains usable with mutations |
| `value: own T` | ownership transfer | non-copy source is moved; copy source remains usable |

Bare parameters are declaration-stable shared parameters:

```aurora
def inspect[T](value: T):
    print(value)
```

The convention is selected when a function is declared. It does not change
when a generic `T` is later specialized to a copy type or when a concrete copy
type is used directly.

### Copy types

This proposal recommends amending ADR-0006 so a bare declaration-known copy
type is a logical shared parameter:

```aurora
def inspect_count(count: int64):
    print(count)
```

The ABI may still pass the copied bits directly, but the source contract
retains shared access through completion of the call. A later sibling
expression may take another shared access, but may not mutate or consume an
overlapping place while the earlier input remains selected. This deliberately
changes current bare-copy snapshot behavior and makes today's explicit
`borrow int64` contract the single bare spelling.

`value: mut int64` remains distinct because it writes through an exclusive
caller place. `value: own int64` is also a distinct callable capability even
though taking a copy does not invalidate the caller's binding. Trait
conformance and operator dispatch compare these logical capabilities, not
whether the current ABI happens to pass copied bits.

Universal logical sharing is necessary if `borrow` is removed completely.
Otherwise a generic trait method declared with bare `T` resolves shared, while
an implementation specialized to a copy type resolves its bare parameter by
value; the implementation then cannot spell the shared contract. Current
builtin APIs also deliberately use shared capability metadata for copy inputs
such as indexes, timeouts, status codes, and random bounds. One bare shared
rule keeps user declarations, specializations, builtin signatures, analysis,
and editor rendering consistent.

### Method receivers

The same three forms apply to receivers:

```aurora
def name(self) -> String:
    return self.name.clone()

def rename(mut self, name: String):
    self.name = name

def into_name(own self) -> String:
    return self.name
```

This replaces the current `borrow mut self` spelling with `mut self`. Current
`borrow self` becomes the already-preferred bare `self`. Bare and `own`
receivers remain different trait capabilities even for a copy class: bare
retains a logical shared receiver for the call, while `own` requests a value
from which the implementation may consume fields. Copying the receiver bits
does not make those contracts interchangeable.

### Trait declarations and implementations

Trait methods and implementations use exactly the same spellings. A method
implementation must match the trait's logical capability. Bare, `mut`, and
`own` remain distinct for copy, non-copy, and unresolved types. Analysis and
editor tooling render the declared spelling while checking that resolved
capability. A specialization never changes the contract selected by the trait
declaration.

## Defaults and temporaries

Existing default-argument rules carry forward:

- a bare shared parameter may have a default; its temporary lives through the
  call
- an `own` parameter may have a default; the call consumes the temporary
- a `mut` parameter may not have a default

A defaulted mutable target would be a caller-invisible temporary, so every
write would be lost. The diagnostic should continue to recommend requiring a
caller argument or taking `own T` and returning the changed result.

A bare parameter may accept a temporary whose lifetime is extended through the
call. A `mut` argument must resolve to an addressable mutable place. An `own`
argument may be a place or a newly produced value.

For this proposal, a mutable argument place is a mutable name or supported
member path. `vec[index]` and `map[key]` do not become mutable-loan places and
must be rejected as `mut` arguments. Supporting them would require an
evaluate-once indexed-place identity, key ownership, overlap rules, live
writeback, out-of-bounds behavior, and abnormal-exit semantics that Aurora 0.1
does not have. Ordinary indexed assignment keeps its existing contained
read/replace operation and is not evidence that an indexed live mutable access
exists.

### Task-start capture boundary

The caller-preservation table above describes ordinary synchronous calls.
Starting a task is a separate retaining boundary: `TaskGroup.start` and
`start_soon` move every non-copy capture into task-owned storage before the
child runs, even when the target function declares that capture with a bare
shared parameter. The child then borrows its task-owned capture. A copy capture
is copied, and a mutable capability remains rejected because it cannot write
through to the caller across the task boundary.

The task-start signature and diagnostic must continue to make that outer
ownership transfer visible. Removing the `borrow` keyword must not make a
retaining asynchronous boundary look like an ordinary synchronous shared
call.

## Other ownership-bearing syntax

The keyword should not be removed from parameters while surviving as an
unrelated ownership modifier elsewhere. If this proposal is ratified, each
ownership-bearing construct needs an explicit decision and migration.

### Place iteration

Recommended syntax:

```aurora
for item in values:
    inspect(item)

for item in mut values:
    update(item)

for item in own values:
    consume(item)
```

The migration preserves the existing iteration model rather than deriving a
new one from the shorter spelling:

- bare `Vec[T]` and `Set[T]` iteration is shared; the selected collection
  remains owned by the caller and non-copy elements are shared
- `for item in mut values` requires an addressable mutable `Vec[T]` place,
  yields mutable element access, and preserves the existing per-iteration
  writeback rules
- mutable Set iteration remains rejected; mutate a Set through its collection
  operations outside iteration
- `for item in own values` accepts a Vec or Set place or temporary, consumes
  that collection, and yields owned elements
- every iterable expression is evaluated and selected exactly once before the
  first iteration, as required by ADR-0017
- a temporary selected for bare iteration lives through the complete loop; a
  selected place cannot be retargeted by rebinding its source inside the body

Range iteration accepts only the bare form. It yields independent copy `int32`
values, so `mut` has no place through which to write back and `own` has nothing
to transfer. Both explicit modifiers are therefore rejected with `AU3004` and
a teaching diagnostic that explains the value-iteration model and suggests
`for item in range(...):`. A future iterable type must explicitly declare
whether it is place traversal, value iteration, or a receive-like operation;
the checker must not silently guess from whether the expression is a place.

Queue iteration keeps its existing carve-out. It is a receive operation rather
than place traversal, so only this form is valid:

```aurora
for item in queue:
    consume(item)
```

Each received item is already owned. `mut` and `own` queue modifiers should be
rejected with a diagnostic explaining why they have nothing to modify. The
Queue handle is copied and selected once at loop entry; rebinding the source
binding does not retarget the active receive loop.

### Pattern matching

Removing `borrow` requires a decision about `match`, whose current bare form is
consuming. The recommended consistent surface is:

```aurora
match value:       # shared match
    ...

match mut value:   # exclusive mutable match
    ...

match own value:   # consuming match
    ...
```

This is a larger semantic migration than the parameter spelling change.
Existing bare matches already parse, so changing their meaning cannot be
detected by a retired-keyword diagnostic. Every existing bare match, including
a copy scrutinee and a non-copy match that does not visibly move a payload,
must become `match own` to preserve current value-snapshot or consuming
behavior. This source flip therefore requires the compatibility and edition
decision below plus an automated migration check.

The recommended match rules apply equally to statement and expression
matches:

- the scrutinee is evaluated exactly once
- bare matching preserves a non-copy scrutinee and exposes shared non-copy
  payload bindings; those bindings may be read but not moved, retained,
  inserted into owned storage, returned as owned values, or captured by a task
- a temporary selected by a bare match lives through the selected arm and,
  for a match expression, through evaluation of its result
- a bare declaration-known copy scrutinee may be represented by copied bits,
  but remains logically shared through the selected arm; copy payload bindings
  are ordinary copies
- `match mut place` requires one addressable mutable place, exposes mutable
  non-copy payload bindings, preserves the current overlap, invalidation, and
  disjoint-sibling rules, and reconstructs the enum into the original place
  on a writeback-producing arm exit
- `match mut vec[index]` and `match mut map[key]` are rejected under the same
  indexed-place boundary as mutable call arguments
- `match own value` accepts a place or temporary, consumes a non-copy
  scrutinee exactly once, and exposes owned payload bindings without a hidden
  clone
- `match own` on a copy scrutinee takes a copy and leaves the source usable;
  the explicit modifier still records the consuming capability of the match
- `match own holder.field` partially moves that field; the overlapping
  ancestor remains unusable until the exact moved path is reinitialized under
  the existing partial-move rules
- payload bindings remain arm-local and cannot outlive their capability or
  retain stale access after the scrutinee or an overlapping ancestor changes

Arm selection and owned extraction are two separate phases. Every arm's
discriminants and refutable nested structure must be probed without consuming
the private scrutinee. Only after one complete pattern is selected may the
runtime destructively extract that arm's owned bindings. In particular, two
arms beginning with the same outer variant cannot let a failed nested pattern
move the outer payload before the later arm is considered. Neither backend may
use speculative extraction followed by cloning or reconstruction as rollback.

Aurora 0.1 currently specifies mutable-match reconstruction on normal arm exit.
Before implementation, the ratification ticket must explicitly decide each
early `return`, `break`, `continue`, and propagated-error path: either perform
writeback before leaving or reject a path that could otherwise discard a
successful mutation. No backend may silently lose a mutation. Nested mutable
matches must retain the current overlap rejection, and both backends must make
the same writeback decision.

If the bare-match default is not changed, a separate shared-match spelling
would still be required. Introducing a replacement such as `ref` would weaken
the goal of having one three-form capability model, so it is not recommended.

### Local bindings and assignment

This proposal does not change ordinary local ownership:

- assigning an owned non-copy value to another binding remains a move
- assigning a currently shared non-copy parameter, receiver, match payload, or
  bare-loop element to a local binding creates a scoped shared alias with the
  same root and provenance; it does not create an owned value or a hidden clone
- `mut name = value` continues to mean that the local binding may be updated
- explicit cloning remains the way to produce an independent owned value

`mut T` is a callable capability annotation, not a general reference type.
Aurora 0.1 does not gain first-class references or lifetime syntax from this
proposal.

A scoped shared alias may be read and copied into another scoped shared alias,
but may not be moved, returned as owned, inserted into owned storage, captured
by a task, or retained after its source loan ends. An alias of a bare-loop
element cannot survive the current iteration. Reassigning the alias is not
permitted: shared aliases remain non-assignable under Aurora's current local
mutability rules. Their access ends at last use or scope exit. Mutating or
consuming an overlapping source remains rejected while a live alias can still
be used. Alias provenance must survive member projection and control-flow
joins conservatively, and MIR and direct execution must preserve the selected
runtime identity without deep cloning.

Local aliases derived from a `mut` parameter, `mut` receiver, mutable-match
payload, or mutable-loop element are rejected in this proposal. Current
lowering cannot represent a write-through local reborrow consistently: a
separate alias can silently mutate a clone and lose the caller-visible write.
The diagnostic should recommend mutating the original place directly or
passing it to another `mut` parameter. Supporting mutable local reborrowing
later requires first-class place identity, exclusive alias lifetime, source
inaccessibility while the alias lives, write-through or writeback on every
exit, and backend parity.

The implementation must retain the current
`borrowed_noncopy_local_alias.au` shared behavior under the new spelling and
add parameter, receiver, match-payload, and loop-element tests for shared
identity, escape, overlap, last-use behavior, non-assignability, and backend
parity, plus exact rejections for every mutable-source alias category.

### Positions that do not gain capability prefixes

The three-form syntax belongs to parameter/receiver declarations and to the
explicitly listed loop and match selectors. It does not turn `mut` or `own`
into a general type constructor:

- call arguments remain ordinary expressions; the callee declaration decides
  whether an argument is read, mutated, or consumed
- class fields and enum payload declarations remain owned storage
  destinations and do not accept `mut T` as a field or payload type
- constructors, collection insertion, assignment, task capture, supervisor
  retention, and `with` resource acquisition keep their existing explicit
  ownership rules
- ordinary return annotations name an owned result type and do not accept
  `mut T` as an escaping mutable reference
- `own` at a loop or match selector remains an operation on that selected
  value, not part of its static type

Diagnostics should identify the valid capability-bearing positions rather than
letting a misplaced prefix fall through to an unrelated type error.

### Resource protocol

The exact user-defined resource protocol changes spelling from:

```aurora
def close(borrow mut self) -> None:
    ...
```

to:

```aurora
def close(mut self) -> None:
    ...
```

All other `with` rules remain unchanged: resource acquisition consumes the
resource expression into a fresh mutable managed binding; cleanup occurs once
on every maintained exit; the binding cannot be moved while cleanup is active;
and user resources must still meet the protocol's class, genericity,
parameter, and return-type restrictions. Compiler diagnostics, the LSP, and
MIR/direct cleanup parity tests must recognize only the new spelling after the
migration window.

### Returns

Aurora 0.1 accepts borrowed-return declarations and checks their source
provenance. Calls returning a copy type materialize an ordinary copy; calls
that would produce a non-copy borrowed result are rejected under ADR-0009.
Borrow labels also connect multiple parameter sources to a return-source slot
and reserve the contract intended for Phase 6.

Removing the syntax is therefore a semantic decision, not a purely mechanical
rename. The recommended migration is to remove shared and mutable
borrowed-return source spellings rather than reinterpret `mut T` as a returned
reference:

- a copy-valued `-> borrow[source] T` or `-> borrow mut[source] T` becomes an
  ordinary owned `-> T`; the returned expression still produces a copy
- source labels used only by that removed copy return disappear
- a non-copy borrowed-return declaration must be redesigned to return a clone,
  index, handle, or owner operation, even though Aurora 0.1 already rejects
  materializing its result at a call
- a label shared by multiple parameters has no replacement in the proposed
  surface because the reserved live-alias contract is intentionally removed

Functions return owned values by default. Code that needs to expose internal
data should return a copy, clone, index, handle, or owner operation. A later
first-class loan design must specify place identity, lifetime relationships,
and escape rules explicitly rather than inheriting them accidentally from this
syntax cleanup. Ratification must separately decide whether this proposal
amends or supersedes ADR-0009 and accepts the loss of its Phase-6 reserved
contract.

### Future callable captures

ADR-0013 is not merely a terminology dependency. It reserves borrowed closure
captures until live-loan tracking exists. If `borrow` is retired first, that
future contract must be amended: a capture that remains inside a proven loan
lifetime uses the same implicit bare shared capability, while a move-only
`FnOnce` capture and every non-copy capture crossing a task boundary remain
explicit ownership transfers. The later callable design must make capture mode
visible or deterministically infer it; it cannot depend on removed
`borrow`-prefixed source syntax.

This ADR does not implement closures or live loans. Ratification must record
whether ADR-0013 is amended along the rule above or whether its shared-capture
roadmap is superseded by a separate design.

## Terminology and internal representation

Public teaching material may describe the model as:

- shared access
- mutable access
- ownership transfer

Compiler internals may retain concepts such as `Borrow`, `BorrowMut`, loans, and
receiver kinds where those names accurately describe implementation behavior.
Renaming internal enums is optional and must not be mixed into the source
migration merely for cosmetic consistency.

The MIR and native ABI still need three distinct conventions:

- shared/read-only input
- exclusive mutable input
- value/owned input

The syntax change does not authorize hidden deep clones. `own` must remain a
real transfer, and shared access must not acquire an unbounded clone as a
substitute for borrowing.

## Source migration

The mechanical source mapping is:

| Current source | Proposed source |
| --- | --- |
| `value: borrow T` | `value: T` |
| `value: borrow mut T` | `value: mut T` |
| `borrow self` | `self` |
| `borrow mut self` | `mut self` |
| user resource `close(borrow mut self)` | `close(mut self)` |
| `for value in borrow values` | `for value in values` |
| `for value in borrow mut values` | `for value in mut values` |
| `match borrow value` | `match value` |
| `match borrow mut value` | `match mut value` |
| current consuming `match value` | `match own value` |
| current bare copy parameter whose snapshot must remain | `value: own CopyType` |
| `value: borrow[label] CopyType` used by a copy return | `value: CopyType`; remove the label |
| `-> borrow[label] CopyType` | `-> CopyType` |
| `-> borrow mut[label] CopyType` | `-> CopyType` |
| non-copy borrowed return | redesign around an owned result, handle, index, or owner operation |

The table is mechanical only where the old and new capability are equivalent.
Rewriting an explicit shared borrow of a declaration-known copy type to bare
is capability-preserving under the proposed universal shared rule. The
non-mechanical copy case goes in the other direction: every current bare copy
parameter is a value snapshot and would silently become a logical shared
parameter without a token change. A syntax-aware migrator must rewrite it to
`own CopyType` when exact snapshot and overlap behavior must be preserved, or
flag it for review. Builtin signatures and trait implementations need the same
inventory even when their capability comes from semantic metadata rather than
parsed source.

The compiler should recognize retired `borrow` forms long enough to emit
targeted migration diagnostics. The recommended compatibility policy is:

1. ship the new grammar atomically while keeping `borrow` as a reserved retired
   keyword for one announced compatibility release
2. parse the legacy ownership productions only far enough to emit their exact
   replacements or redesign guidance; never accept them as normal aliases
3. after that window, a later language-version decision may release `borrow`
   as an ordinary identifier

This avoids two permanent spellings and gives old source a deterministic
diagnostic. If ratification instead makes `borrow` an identifier immediately,
the acceptance criteria for legacy diagnostics must be narrowed: a contextual
legacy recognizer needs an explicit precedence rule so legitimate identifiers
named `borrow` are not misparsed.

Example diagnostics:

```text
`borrow T` was removed; shared access is the default, so write `T`
```

```text
`borrow mut T` was removed; write `mut T` for mutable access
```

```text
bare `match` is shared; write `match own value` to consume payloads
```

Because current bare matches and bare copy parameters contain no retired token,
their migrations cannot rely on parser recovery. The source migrator or
compatibility lint must insert `own` on every current bare match when
preserving current copy-snapshot or non-copy consuming behavior, and inspect
every bare copy parameter whose value-snapshot capability must remain `own`.
The implementation must not silently recompile either class of unchanged
source under different ownership sequencing.

## Implementation plan

### 1. Ratification and inventory

- Ratify the recommended amendment from ADR-0006's declaration-known copy
  snapshot rule to universal logical sharing, or choose a different
  representable shared-copy spelling before removing `borrow`.
- Decide the `match` default explicitly.
- Decide mutable-match writeback on every early control-flow exit.
- Decide whether removing borrowed-return labels amends or supersedes
  ADR-0009.
- Decide the retired-keyword compatibility window.
- Decide the pre-1.0 language-version and package-edition boundary.
- Inventory every grammar production and maintained source file containing
  `borrow`.
- Inventory every current bare match, including copy scrutinees and non-copy
  matches without a visibly moved payload.
- Inventory every current bare copy parameter, receiver, builtin signature,
  and concrete trait implementation whose snapshot capability would change.
- Record the amendment and supersession relationships to all affected ADRs,
  including ADR-0013's future capture contract.
- Keep this work outside the active Batch 2 ticket family until that checkpoint
  is complete.

### 2. Lexer, parser, AST, and source migration

- Parse `mut T` in parameter capability position.
- Parse `mut self`.
- Parse `in mut expression` for place iteration.
- If ratified, parse `match mut expression` and `match own expression`.
- Retain contextual recognition of old forms solely for migration diagnostics.
- Build a syntax-aware migrator or compatibility lint for the silent bare-match
  and bare-copy semantic flips plus the non-mechanical borrowed-return cases.
- Update AST serialization snapshots and, if Aurora has a formatter by the
  implementation date, its round-trip tests.
- Ensure `mut` remains unambiguous with mutable local bindings.

### 3. Semantic analysis

- Map bare, `mut`, and `own` declarations to the existing three receiver kinds.
- Preserve declaration-stable generic conventions.
- Apply universal logical sharing to copy inputs without treating ABI copying
  as proof that bare and `own` capabilities are equivalent.
- Enforce mutable-place, temporary-lifetime, exclusivity, and move rules.
- Preserve scoped shared-local provenance without introducing hidden clones,
  reject every mutable-source local alias, and reject indexed mutable-loan
  places in this migration.
- Preserve default-argument restrictions.
- Update trait compatibility, method lookup, operators, task captures,
  collection insertion, `with` protocol checking, and retaining builtin
  metadata.
- Implement the ratified loop, Queue, and match rules.
- Reject capability prefixes in fields, payloads, return types, call arguments,
  and all other positions where this ADR does not introduce them.

### 4. MIR, runtime, and native backend

- Verify that source syntax lowers to the same explicit MIR conventions.
- Pin true moves for `own` and mutation writeback for `mut`.
- Prove that shared access does not rely on hidden deep clones.
- Lower consuming matches as non-destructive pattern selection followed by
  destructive extraction only for the selected arm.
- Preserve shared-local alias identity and provenance across both backends.
- Keep interpreter/MIR and direct-native behavior in parity.
- Audit FFI and host-builtin adapters that consume passing metadata.

### 5. Diagnostics and tooling

- Add exact migration diagnostics and fixes for every retired spelling.
- Update the currently maintained completion details, hover signatures,
  go-to-definition tests, TextMate grammar, snippets, and syntax highlighting.
- If signature help, semantic tokens, or an Aurora formatter exist by the
  implementation date, update them in the same pass; creating those unrelated
  features is not silently part of this syntax migration.
- Update the VS Code extension and language-server packaging tests.
- Ensure generated API signatures render only bare, `mut`, and `own`.

### 6. Maintained source and reference

- Migrate compiler fixtures, examples, tutorials, the Manual, package READMEs,
  architecture docs, and work records in the same change family.
- Update the normative grammar and ownership chapters.
- Add an automated stale-spelling check over maintained source.
- Preserve old spellings only in historical ADR context and explicit migration
  documentation.

### 7. Verification and release boundary

- Run all parser, semantic, MIR, native, fixture, LSP, extension, reference,
  documentation, coverage, and backend-parity gates.
- Test a clean migration from representative old source.
- The recommended pre-1.0 path is one atomic source flip within edition
  `2026`, before Aurora 0.1 is declared stable. If edition `2026` has become a
  stable compatibility promise by implementation time, defer the semantic
  flip to a new edition instead.
- Bump the compiler semantic-interface/cache schema and include it in every
  Phase-4 build-cache key. Invalidate cached AST, checked-program, MIR, native,
  dependency-interface, and LSP analysis artifacts so ownership metadata from
  the old grammar cannot cross the migration.
- Call the breaking change, migration command, compatibility window, and
  borrowed-return contract removal out in release notes.

## Required test matrix

At minimum, tests must cover:

- bare, `mut`, and `own` parameters for copy and non-copy types
- declaration-known bare copy loans, explicit `own` copy snapshots, overlap
  behavior, and source-ordered call interactions
- generic declarations specialized with both copy and non-copy types without
  changing their declaration-resolved convention
- generic traits specialized to concrete copy types, concrete implementations,
  operator traits, and builtin shared-copy signatures with no unspellable
  contract
- bare, `mut`, and `own` receivers and trait conformance, including their
  distinction on copy classes
- mutable name/member-place acceptance plus exact temporary, immutable-place,
  Vec-index, and Map-index rejection
- shared and owned default values plus rejected mutable defaults
- ownership after calls, including use-after-move diagnostics
- shared/mutable/owned Vec iteration, shared/owned Set iteration, exact mutable
  Set rejection, and temporary lifetime plus one-time source selection
- bare Queue receive ownership and exact rejection of `mut` and `own`
- bare Range iteration yielding copy `int32` values plus exact `AU3004`
  rejection and bare-form guidance for both `mut` and `own`
- statement and expression forms of every ratified match mode, with one-time
  evaluation, temporary lifetime, copy scrutinees, payload move restrictions,
  mutable place rejection, reconstruction, stale-binding invalidation, nested
  overlap, projected-field partial moves, and every early arm exit
- repeated-outer-variant consuming patterns that prove every arm is selected
  non-destructively before the chosen payload is moved
- nested places, enum payloads, class fields, collection elements, and indexing
- scoped shared local aliases covering identity, member projection, overlap,
  last-use behavior, non-assignability, escape/storage/task rejection,
  control-flow joins, and the per-iteration lifetime of a shared loop-element
  alias
- exact mutable-local-alias rejection for `mut` parameters, receivers, match
  payloads, and loop elements, with no silent lost-write backend path
- retaining builtins, task captures, operator traits, and the exact
  `close(mut self) -> None` resource protocol on normal and abnormal cleanup
- rejection of capability prefixes in fields, enum payload types, ordinary
  returns, calls, and other non-capability positions
- parser recovery on every retired spelling; formatter idempotence if a
  formatter exists by implementation time
- copy and non-copy borrowed-return migrations, parameter/return labels, and
  the ratified ADR-0009 disposition
- exact LSP signatures, migration diagnostics, identifier precedence during
  the compatibility window, and syntax-aware bare-match plus bare-copy
  migration proofs, including copy-match overlap behavior
- MIR/direct parity and no-hidden-clone ownership probes
- cache-schema invalidation across the old and new ownership metadata

Tests must pin observable behavior or a diagnostic. They must not exist only to
execute new branches for coverage.

## Acceptance criteria

The proposal is complete only when:

- the ratification questions below have recorded decisions
- all maintained source uses the new spellings
- old spellings produce precise migration diagnostics and are not accepted as
  normal syntax during the ratified compatibility window
- no unchanged current bare match, including a copy scrutinee, can silently
  acquire the new shared meaning; migration or edition selection makes the
  change explicit
- every bare parameter is a logical shared capability and preserves the caller
- declaration-known copy parameters retain shared overlap sequencing even if
  the ABI passes copied bits
- `mut` writes through an exclusive caller place
- `own` performs a true ownership transfer
- copy specialization does not change a generic declaration's convention
- generic trait specializations and builtin copy inputs have representable
  source-level capabilities
- shared local aliases preserve identity and cannot escape as owners
- mutable-source local aliases are rejected until write-through live loans
  exist
- consuming matches select an arm before destructively extracting payloads
- all ownership-bearing syntax follows the ratified model
- borrowed-return contracts and the user-resource protocol have complete
  migrations
- old compiler interfaces and Phase-4 cache entries cannot be reused under the
  new semantics
- compiler, MIR, direct backend, language server, extension, examples,
  tutorials, and normative reference agree
- the full repository gate and frozen coverage policy pass

## Ratification questions

1. Is the recommended ADR-0006 amendment to universal logical shared bare
   parameters ratified? If not, which explicit shared-copy spelling remains so
   generic trait specializations and builtin signatures stay representable?
2. Does bare `match` become shared, making `match own` the consuming form?
3. How does mutable-match reconstruction behave on `return`, `break`,
   `continue`, and propagated-error exits?
4. Is the silent bare-match semantic flip protected by a syntax-aware
   migration/lint in edition `2026`, or deferred to a new edition?
5. Is `borrow` reserved for one compatibility release to enable targeted
   diagnostics, or removed from the keyword set immediately?
6. Does removing borrowed-return and label syntax amend or supersede ADR-0009,
   and is losing its reserved Phase-6 live-alias contract accepted?
7. Should public documentation avoid the word “borrow” entirely, or retain it
   as explanatory ownership terminology while removing only the keyword?
8. Are explicit shared spellings intentionally unavailable, so there is only
   one canonical source form?
9. Does the change ship as one atomic language migration or as parser/tooling
   support followed by a separately announced source flip?
10. Does ADR-0013 retain future shared closure captures under an implicit bare
    capture contract, or is that part of the callable roadmap redesigned?

## Non-goals

- first-class reference types
- user-written lifetimes
- borrowed non-copy returns
- changing ordinary local assignment from move semantics
- weakening exclusive-access or use-after-move checks
- using hidden cloning to simulate shared access
- beginning this migration inside an unrelated in-progress Batch 2 ticket
