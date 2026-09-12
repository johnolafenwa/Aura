# Batch 1 design checkpoint: callable and type foundations

Status: **Ratified; phase 1 implementation in progress**

Date: 2026-09-08. Source baseline: `de3d6cc44a376502f2aaf3f3ef8faba59c523884`.

This is the ratified design checkpoint for [roadmap Batch 1](14-priority-roadmap.md#priority-batches).
The checkpoint itself specifies the target. The user accepted all recommended
options except **Q6: B** and **Q20: B**, with a Batch 2 follow-up attached to
**Q23: A**. The [ratification record](#ratification-record) and
[questionnaire](#ratification-questionnaire) record those answers. Batch 1
phase 1 records them in ADR-0052 and ADR-0058 before code changes. The design below
incorporates the overrides; it describes the accepted target, not current
compiler behavior.

The ten [Approved Decisions](14-priority-roadmap.md#approved-decisions) remain
binding and unchanged: explicit union boundaries, deterministic normalization,
one optional spelling, structural task isolation, the three callable call kinds,
and preservation of exposed calling contracts are constraints on the choices.
There is no vote here to reintroduce mixed-literal inference, hidden cloning,
garbage collection, identity equality, or a compatibility path for `Option`.

The inputs are [ADR-0052](decisions/0052-anonymous-closed-union-types.md),
[0058](decisions/0058-first-class-callables-and-binding-contracts.md),
[0037](decisions/0037-expression-closures-and-value-capture.md),
[0038](decisions/0038-place-based-loans-and-views.md),
[0013](decisions/0013-callable-sequencing-and-ownership.md),
[0015](decisions/0015-explicit-and-default-argument-order.md),
[0051](decisions/0051-import-aliases-and-keyword-only-parameters.md),
[0057](decisions/0057-clean-slate-pre-adoption-policy.md), and
[0064](decisions/0064-native-backend-strategy-and-codegen-boundary.md), together
with the open conflicts in ADR-0038/0058/[0061](decisions/0061-collection-element-loans-and-slice-views.md).
The implemented baseline is described in the Manual's
[Closures](../docs/manual/closures.md), [Types](../docs/manual/types.md),
[Functions](../docs/manual/functions.md),
[Generics and Traits](../docs/manual/generics-and-traits.md),
[Enums and Match](../docs/manual/enums-and-match.md),
[FFI](../docs/manual/ffi.md), and [Current Limits](../docs/manual/current-limits.md).
The maintained usability oracle is [tool_runner version 0](../examples/agents/tool_runner/src/main.au).

All code and grammar fences in this proposal are plain `text`, including the
complete candidate program. They are design examples, not executable reference
claims. No new reserved keyword is proposed. `type` and `is` use complete
contextual productions; `Callable`, `TaskCallable`, `Lookup`, and `Poll` are
proposed builtin type names, not lexical keywords. Existing `mut`, `own`,
`def`, `as`, `None`, and `*` supply the other notation.

## Ratification record

On 2026-09-08 the user accepted **all recommended, except Q6: B and Q20: B**.
For **Q23**, the user accepted **A** and required the Batch 2 element-loan
design to revisit an app-facing `V | None` form of `dict.get`.

- **Q6: B:** allow symmetric union/member equality through unique-member
  contextual injection, retaining literal ambiguity checks. Hashing delegates
  to the active member so cross-type equal values have equal hashes.
- **Q20: B:** add no `base.wrap(body)` intrinsic. Handwritten wrappers supply
  inner arguments or deliberately invoke inner defaults and define their own
  public default policy.
- **Q23: A, with follow-up:** Batch 1 uses `Lookup`/`Poll` as specified in D.
  Batch 2 revisits the app-facing optional dictionary lookup surface together
  with element loans, including present-None distinctions and result lifetime.
- **All other questions: A.** The original options and recommendations from
  proposal commit `fe9c6c0` remain in the questionnaire, with a separate
  **Ratified** line recording the selected answer to each question.

The ratification record is complete. The active Batch 1 phase 1 task covers
ADR-body reconciliation and compiler, runtime, example, Manual, and editor
implementation; it leaves phase 2 Option removal for a later task.

## A. Unions and narrowing

### A1. Identity, normalization, and unit None — Q1

Retain ADR-0052's precedence: `|` binds below every other type constructor;
`list[int64 | str]` has a union element, and a callable returning a union uses
`def() -> (int64 | str)` when parentheses disambiguate its extent. Expand
transparent aliases, recursively flatten unions, deduplicate by resolved type
identity, then order by the canonical structural type key with `None` last.
Nominal keys contain defining module identity and canonical type arguments;
tuple/callable keys recursively encode their complete contracts. No source
order, filesystem traversal order, alias spelling, or pointer address enters
the key. Version the key's encoding with the checked interface.

**Recommend collapse to the sole member**, rather than rejecting a union that
normalizes to one member. Thus `int | int64` is `int64`, `None | None` is unit
`None`, and `(T | None) | None` is `T | None`. The alternative is an error after
deduplication; it makes generic substitution and generated aliases fragile.
The empty union has no source form, and a trailing or missing operand is a
parse error. An internally empty flow refinement means an unreachable path,
not a new source `Never` type.

Standalone `None` keeps the implemented unit type/value behavior: `x = None`
is a unit binding, not an inferred optional. This reconciles ADR-0052's
unconstrained-`None` baseline with the current language. With an explicitly
expected optional, `None` selects its absence member. `return` without a value
and implicit fallthrough still belong only to functions declared to return
unit `None`; an optional return requires an explicit `return None`.

One-member collapse is representation identity too: no wrapper, tag, extra
drop, or different property classification remains. Nested optional
normalization deliberately loses multiplicity of absence. Section D supplies
nominal outcomes for APIs that must retain that distinction.

### A2. Injection and a member annotation inside an expression — Q2

An already typed union can be used without another annotation, but unrelated
values never infer a new union. The allowed introduction boundaries are an
annotated binding, declared parameter/result, explicit generic specialization,
and explicitly typed collection element or aggregate field. Expected types
flow through parentheses and the value-producing arms of conditional/match
expressions, through a lambda result contract, and into the elements/payloads
of an explicitly typed constructor. They do not infer the private parameter
types of an intervening call from its use, or flow backward through an
unannotated arithmetic computation to invent a union.

Probe literal contextual typing against each normalized member without
executing the expression. Accept exactly one successful member. Zero matches
is `AU2010`; several is `AU2011`. For `int64 | float64`, the literal `1` fits
both and is ambiguous; neither canonical order nor the normal `int64` default
breaks the tie. `1.0` selects `float64` when it is the only matching floating
member. Non-literal `int64` does not promote to `float64`. `bool` is distinct
from integers. Injection evaluates once, then copies or moves the active
payload under the destination's existing capability rule.

**Recommend existing numeric casts plus hoisted typed locals**, with no new
expression-ascription grammar. The alternative extends `as` to a static type
ascription for every value; that overloads today's checked numeric conversion
and could be mistaken for a runtime union downcast.

```text
type Number = int64 | float64
integer: int64 = 1
number: Number = integer
other: Number = 1 as float64

# If an expected type does not reach a nested non-numeric expression:
items: list[int64 | str] = [1, "two"]
consume_outer(make_request(items))
```

A named generic helper with an `own T` parameter and explicit specialization
is another existing boundary when hoisting is awkward. In an expression-only
lambda, use such a helper or a named function; this proposal does not add
local declarations to lambda bodies. There is no general union cast, implicit
member-set widening, automatic container conversion, or union construction
from an unannotated branch join.

### A3. Stable places and invalidation — Q3

**Recommend refinement facts over the existing place model**, not hidden
payload copies. A fact contains a root/generation, fixed projection path,
possible-member set, and any containing loan/arm identity. These are analysis
facts, not new owned values or user-visible lifetime parameters.

| Expression being tested | Eligibility and narrowed access |
| --- | --- |
| Owned immutable or `mut` local | Stable until move/rebind/reinitialization; retain its access capability |
| Bare, `mut`, or `own` parameter/receiver | Stable according to its declared capability; generic specialization never upgrades a bare parameter to ownership |
| Fixed class field or tuple position through shared access | Stable shared projection; no non-Copy extraction or mutation |
| Fixed field through mutable access | Stable while exclusivity holds; mutable payload access is contained by that access |
| Shared view | Refine the pointee with shared capability inside the view's region |
| Mutable view | Refine the pointee with exclusive capability; suspend parent access during a reborrow |
| Existing arm-local payload place | Refine within that arm only; never return a view of it |
| Index, dictionary key, computed property, repeated call, arbitrary temporary | Test may produce `bool`, but no fact attaches to a later evaluation; bind an owned result to a local first |

The alternative narrows only locals/parameters and requires snapshots or
explicit match blocks for fields/views. It is simpler, but leaves the
roadmap's existing place foundation poorly integrated.

Testing a place alone does not hold a new loan for the whole branch. Reading
or borrowing its payload establishes the ordinary contained access. An owned
non-Copy root may be consumed after narrowing to one member: that consumes the
**whole original union**, marks its slot moved, and yields the active owner.
A shared/mutable parameter or view cannot do this. An owned field obeys the
existing partial-move restrictions; narrowing does not waive them.

Invalidate facts at an assignment to the place or an ancestor, a move/drop,
or acquisition of overlapping whole-union mutable access. Evaluate an
assignment's right side first, then replace the old value and advance the
generation; facts about the old value cannot survive. A new known member can
seed a new fact. Proven-disjoint sibling field mutation preserves the fact.

An opaque call invalidates facts for places supplied as `mut` or `own`,
including aliases and ancestor footprints, before user defaults/body can run.
Shared calls preserve tag facts only where the existing loan rules prevent
overlapping mutation; this is not an inference of whole-function purity.
Calls through retained loan closures apply their recorded capture footprints.
An ordinary call with no access route to a local does not invalidate it merely
because the callee's body is unavailable. Module constants are not mutable
global roots.

A `mut Member` call through a narrowed payload locks the parent tag for the
call and cannot retag the union; it invalidates mutable facts *inside* that
payload. Passing the original place to `mut Union` explicitly exposes the
whole place and kills its tag fact. This restoration of the declared root
type is not subtyping between unrelated unions.

Loan end removes facts dependent on that view; mutable reborrow completion
does not restore an old pointee fact without a fresh test. Shared reborrow
completion may retain an unchanged parent fact. Mutation/rebinding while a
payload view is still live is rejected by the loan checker, rather than
merely forgetting its type and allowing stale access.

### A4. Conditions and control flow — Q4

**Recommend short-circuit flow analysis** for `x is None`, `x is not None`,
parentheses, `not`, `and`, and `or`. `is` is contextual in these complete
comparison forms only. Other identity comparisons, `isinstance`, truthiness
unwrapping, and user predicates do not narrow. `None is x` is not a second
spelling in this first grammar. Existing `==`/`!=` do not create narrowing
facts. The alternative implements the two direct tests only and requires
nested `if` blocks for Boolean compositions.

These tests have comparison precedence, above `not`/`and`/`or`; chaining an
`is` test with another comparison is rejected, so write two Boolean terms
explicitly. `is` remains an identifier outside the complete test production.
Keyword-free alternatives are equality tests (which would need an exception
to A6's value-equality eligibility), a method/function such as `is_none()`
(a second absence API instead of the requested Python-shaped test), and an
ordinary match (already useful but not a composable condition). None expresses
the preferred condition while preserving ordinary equality without special
treatment. A contextual production avoids reserving a new identifier globally.

For a test, the true/false successors intersect/remove `None` from the
possible-member set. `not` swaps successors. In `a and b`, check `b` under
the true facts of `a`; in `a or b`, check it under the false facts. Mutations
inside `b` kill facts normally. At a join, union the member sets of reachable
incoming paths for the same live place generation. A fact survives only if
it holds on every incoming path; never invent a broader declared union.

```text
if value is not None and value.len() > 0:
    print(value)

if value is None:
    return None
print(value.len())

while condition:
    current: str | None = read_one()
    if current is None:
        continue
    print(current)
```

`return` and `try` error edges do not join a later statement. `continue`
joins the loop header after iteration loans end; facts there require a fixed
point over the entry and every backedge. `break` joins the after-loop block
with the normal-exhaustion/false-condition path. A `break` on the absent path
does not prove presence after the loop. Unreachable paths contribute no fact
but still receive normal name/type checking; flow reachability grants no
capability escape. An already non-optional place's `is None` test is a constant
false test with once-only evaluation, not an error or an identity operation.

### A5. Patterns, exhaustiveness, and mutable tag changes — Q5

**Recommend ADR-0052 type arms with the existing match modes**:

```text
match value:
    case int64 as number:
        print(number + 1)
    case str as text:
        print(text)
    case None:
        print("missing")

match mut value:
    case int64 as number:
        number += 1
    case _:
        pass
```

`case Type as name` selects one direct normalized member, after alias
expansion. A union-valued alias cannot stand for several arms. For a type
that collapsed to a singleton, its matching type arm is an irrefutable
one-member test; retain ordinary scalar/nominal patterns too. A tagged
nominal payload may contain a union type pattern recursively. No class
destructuring is added. A bare union value cannot be matched by a member's
literal directly: first select its type, then use a guard or nested match.

Guards add no exhaustiveness coverage. An unguarded arm covers its member;
a final `_` or name covers the remainder. Reject duplicate unguarded members,
unreachable arms, nonmembers, and uncovered members. Alternative patterns must
bind identical names, types, and capabilities; `int64 as x | str as x` fails
that rule. No implicit union is inferred for `x`.

Bare match gives shared non-Copy bindings and ordinary Copy values;
`match mut` gives contained write-through bindings; `match own` consumes the
whole union and moves the selected payload after a guard commits. No extra
`mut` after `as` is required. Match-payload views remain arm-local under
ADR-0038. Shared and mutable matches retain their logical source locks even
when the payload is Copy.

A narrowed payload alias can be replaced only with the same member type.
Changing a tag requires assignment to the original whole union place. This
is allowed once every conflicting payload access ends; inside a live match
arm, the scrutinee lock prevents it. In a conditionally narrowed `mut` local,
`value = None` is a whole-place assignment and immediately kills the old
fact. Evaluate the replacement before dropping the old active payload; a
trapping right side leaves the old payload intact. Early exits release
contained views before payload teardown. The alternative restricts type
patterns to shared/owned matching and delays mutable matching, leaving
mutable callers to replace the whole union outside a match.

### A6. Equality and hashing — Q6

**Ratified Q6: B — symmetric union/member equality with contextual injection.**
Two values of the same normalized union are equal iff their tags agree and
their payloads are equal. All members must support equality; adding an
incomparable member removes equality for the union, irrespective of its
current tag. Callable, opaque-handle, and Array restrictions remain structural.
`None == None` retains unit equality.

For `==` and `!=`, when exactly one operand has a union type U, check the other
operand against U using A2's unique-member rule. The already declared union
provides the expected comparison type; the comparison does not infer a new
union. Either operand order is admitted with the same result. A typed member
selects its exact member; a literal that fits multiple members is AU2011 and
requires a member annotation. Thus comparing `int64 | float64` with bare `1`
is ambiguous, while an explicitly int64 operand selects the int64 tag. A
nonmember is rejected, and two different normalized unions remain
incomparable. There is no union widening or container conversion.

The injected operand is a comparison-only typed adaptation. Evaluate each
operand once in source order and use the normal equality access rules;
comparison does not move or clone an owned payload or allocate a union box.
`optional == None`, `None == optional`, and their `!=` forms work when the
entire optional supports equality. These comparisons do not establish
narrowing facts. `is None` and `is not None` remain the tag-only narrowing
tests and work even when another member cannot be compared.

Where a hashing operation is available, derive it only if every member
supports hashing, and **delegate to the active payload's hash without adding
the union type domain or tag**. For a member t that compares equal to a union
value u, `hash(u) = hash(t)`; this notation states the law and does not add a
public hash operation. Equal same-union values also hash equally, including
the existing signed-zero rule of a hashable floating member. Different tags
still compare unequal but may hash alike: collisions are permitted. Nominal
enums such as Lookup retain their own equality/hash rules.

Dictionary/set keys still use their declared key type and ordinary injection
rules; comparison does not add heterogeneous key lookup or relax key
eligibility. No hash value or algorithm is stabilized across runs/releases,
and no new public `Hash` trait or `hash()` API is introduced. Ordering a union
remains unavailable even if all members are orderable. The unselected Q6 A
would require explicit union-typed operands and allow tag/domain-salted union
hashes; those restrictions are not the ratified contract.
The stage 1 ADR-0052 amendment marks its tag-inclusive hashing alternative
unratified and records this payload hash law.

### A7. Generic members and specialization — Q7

**Recommend symbolic type-parameter members followed by normalization after
substitution**. A declared value parameter `V` is a valid member; an
unspecialized constructor such as `list` or `Box` is not a complete type.
Views, capability constructors, module/trait names as values, and FFI-only
pointer/length views remain ineligible. Recursive layouts still require the
existing nominal indirection boundary.

```text
type MaybeValue[V] = V | None

def absent[V]() -> V | None:
    return None

def present[V](value: own V) -> V | None:
    return value
```

`present[int64 | None](None)` and `absent[int64 | None]()` have the same
result value. Specializing `V = None` produces unit `None`. A generic union
never preserves a phantom outer tag to distinguish those results. Use the
tagged APIs in D when that distinction matters.

Generic checking cannot assume distinctness between `V`, `W`, and `None`.
An injection of a value already typed `V` carries a symbolic remapping plan;
when `V` specializes to a union, remap its active tag into the normalized
destination, without a runtime conversion or a nested union wrapper. This
specialization rule is confined to an explicitly declared generic union,
not general subset subtyping.

Generic `is not None` computes an internal non-None refinement of `V`; it
does not assert that `V` itself excludes None. Code may restore that payload
to its originating declared `V` using the recorded remapping, and may use
only operations proved for every remaining alternative by the declared
bounds. This internal fact is not a source type constructor or an inferred
public union. Do not infer `V` by inverting normalization: from an expected
`int64 | None`, both `V = int64` and `V = int64 | None` are possible. Require
another unambiguous argument or explicit specialization.

A generic `case V as value` requires proof that `V` denotes one direct member
disjoint from every other arm. Unconstrained `V` in `V | None` cannot prove
that. Use `case None` followed by a remainder catch-all, or optional tests;
check the body universally before concrete instantiation. Substitution may
not manufacture duplicate arms or new capabilities. Concrete union Copy,
clone, equality, hash, and Transfer derivation inspect all concrete members;
an unresolved member grants no property without an existing obligation/bound.
This does not change declaration-stable parameter passing or the current
conservative classification of generic nominal enums.

The alternative admits optional generic members but forbids their use with
optional substitutions. That forces negative constraints or specialization
failures into ordinary library APIs and conflicts with compositional aliases.

### A8. Trait dispatch and Display — Q8

**Recommend all-member nominal trait dispatch and a separate structural
rendering fallback**. A union satisfies an ordinary trait obligation when
every member satisfies that same specialization, with equal parameter
capabilities, binding contract, result type, and returned-origin contract
after substituting each member for `Self`. A result such as `Self` that becomes
different concrete result types is not coherent; require explicit matching
and an explicitly declared result union. There is no common-name duck typing,
user `impl` on an anonymous union, inferred trait intersection, or implicit
numeric/operator promotion. Mutable trait calls hold the tag lock; owning
trait calls consume the original union. Ordering remains excluded.

Rendering a union reads its active payload once and delegates to the current
structural renderer. It prints the payload alone, or `None`, without a union
wrapper/tag number. When [ADR-0055](decisions/0055-display-trait-and-properties.md)
lands in Batch 4, that same route selects the member's explicit Display or
recursive structural fallback. A fallback permits `print`/`str`/default
f-string rendering without claiming a user type nominally implements the
Display trait. A `T: Display` generic bound still needs nominal satisfaction
on every union member. This matches ADR-0055's distinction between direct
`.display()` availability and fallback rendering, while not implementing
Display, `str(...)`, properties, or print atomicity in Batch 1.

The alternative treats every structurally renderable member as satisfying
the nominal Display bound. It makes generic rendering easier, but changes
trait satisfaction and coherence for the later Display design. No proposal
here claims that shared rendering is free of unrelated side effects.

### A9. Layout, cleanup, interfaces, and niches — Q9

**Recommend a canonical explicit tag plus aligned payload in Batch 1**.
Logical tags are dense canonical-member ordinals. The shared layout plan
chooses the smallest unsigned tag width holding every member, aligns the
payload to the maximum member alignment, uses the maximum member size, and
rounds total size to aggregate alignment. A singleton uses its member layout.
The layout is target-dependent internal Aura ABI, not a C ABI or portable
serialization. No mandatory per-union allocation is introduced: an inline
value stays inline; payload types retain their own allocations/indirection.

Checked interfaces and MIR encode canonical members, tag mapping, ownership
properties, target/layout version, and drop plans. Importers rebuild stale
interfaces/caches; they never guess a tag from source order. Only the active
payload is destroyed, once. A move transfers its drop obligation and clears
the source initialization state. Normal exit, `try`, trap, cancellation, and
partial aggregate construction share the maintained ordered exit actions.
Forced frame reset retains ADR-0038's boundary: host containment may release
runtime ownership, but cannot execute arbitrary source cleanup on dead frames.

The ADR-0052 niche clause survives as permission for a later, centrally
specified optimization, **not independent backend discretion in Batch 1**.
A niche may be enabled only when both backends use the same ABI plan and
preserve member identity, valid values, reported layout metadata, cleanup,
interfaces, and cache identity. The alternative implements nullable-handle
niches internally now, saving a word in eligible cases but adding two physical
representations to the initial parity matrix. The FFI adapter below is a
specified marshalling conversion and does not require an internal niche.

### A10. Nullable results at the FFI boundary — Q10

There are **zero current FFI v0 signatures using Option**: the Manual permits
non-null opaque handles and traps with `AU4005` on a null result. Thus removal
does not require preserving a nonexistent nullable-Option ABI. Nevertheless
the roadmap requires a stated way to represent nullable results.

**Recommend one explicit result-only exception**: an extern result whose
normalized type is exactly a declared opaque handle plus None is marshalled
as one C pointer. No union tag or aggregate crosses C. Null constructs Aura
`None`; non-null constructs the owned opaque wrapper. `-> Handle` continues
to diagnose null as `AU4005`.

```text
extern "C" opaque class Device
extern "C" def find_device() -> Device | None
extern "C" def close_device(device: own Device) -> None
```

This accepts aliases of exactly that shape too. It excludes optional scalars,
strings, returned byte views, nullable parameters, multiple handle alternatives,
general union arguments/results, and callbacks. Those need an explicit C
adapter returning supported scalar status/IDs or a later FFI design. No raw
pointer becomes an Aura value. Opaque wrappers remain non-Copy, non-cloneable,
non-Transfer, and without automatic foreign destructors; a binding must still
call its consuming close function. Mutable-byte writeback precedes result
translation/validation as today. Package opt-in, symbol lookup, synchronous
execution, and the foreign-unwind boundary are unchanged.

The alternative keeps every union out of extern annotations and requires a
reviewed C shim using the existing scalar/status and non-null-handle surface.
That preserves the absolute baseline exclusion, but cannot directly declare
an ordinary nullable-pointer result. The recommendation requires an explicit
ADR-0052/FFI amendment after ratification; it is not already permitted.

## B. Type aliases

### B1. Declarations, generic parameters, scope, and cycles — Q11

**Recommend module-level transparent aliases with existing visibility**:

```text
type ToolValue = int64 | str | None
public type Entries[K, V] = dict[K, V]
type Callback[T] = Callable[def(value: T) -> None]
```

`type` is contextual only at an item position completing
`[public] type Name[parameters] = Type`. `type = value`, a member called
`type`, and a parameter named `type` remain ordinary identifiers. Alias
parameters use the existing names, arity, and trait-bound grammar; bounds are
checked when the alias is specialized. No defaults, higher-kinded parameters,
value parameters, local/class aliases, or recursive aliases are added. The
right side is a type expression, never a runtime expression or constructor
call. Forward references resolve with module items before body checking.

An alias occupies the module item namespace, is private by default, and may
be imported or import-aliased normally. `public type` exports the alias and
its checked expansion; it cannot conceal an otherwise inaccessible type in a
public signature. No general re-export declaration is introduced. Private
aliases may be expanded in a public interface only when every referenced
underlying type is public. Alias use in a value position exposes only an
existing constructor/member of its expansion, including the proposed callable
adapters in C; it invents no scalar or union constructor.

Reject every alias-expansion cycle with a path, including indirect generic
cycles and cycles that grow their arguments. Check the dependency graph of
alias declarations before expansion, so `A[T] = A[list[T]]` cannot diverge.
Nominal recursion still uses a class's existing `indirect` field; an alias
naming that already valid nominal type does not make its field graph an
alias-expansion cycle. The alternative starts with nongeneric aliases only,
leaving common library callback and collection signatures repetitive.

Keyword-free alternatives considered are `Name = T`, which collides with
ordinary value assignment/module constants; `Name: TypeAlias = T`, which
adds a metatype-like marker without improving semantics; and `alias Name = T`,
which is another contextual introducer with less Python familiarity. None
requires a reserved keyword. The recommended contextual `type` uses the
user's preferred spelling without reserving it globally.

### B2. Identity, diagnostics, hover, and checked hashes — Q12

**Recommend transparent semantic identity plus retained presentation metadata**.
Alias chains expand before type equality, union normalization, generic
matching, trait lookup, layout, ABI, and specialization. Reordering union
members behind an alias does not change type identity. Callable aliases retain
the complete call contract, not just parameter/result ABI types.

At a source use, diagnostics print the written alias with its normalized
expansion when needed: `expected ToolValue (= int64 | str | None), found bool`.
Hover displays the visible alias name, type parameters, defining module, and
canonical expansion; go-to-definition on the alias name reaches its declaration.
Imported aliases use the local imported spelling with a qualified origin.
For inferred types with no written alias, use the canonical expansion rather
than searching imports for an arbitrary preferred nickname.

The checked interface carries both a normalized semantic type and exported
alias records. Its semantic type key hashes the expansion; its complete
checked-interface hash also hashes exported alias names, visibility, parameter
contracts, and normalized targets. Renaming an exported alias changes that
interface hash because name lookup changes, while equivalent reordered
expansions share their semantic key. Diagnostic source spans/docs belong to
presentation/source identity, not runtime tags. The alternative erases aliases
entirely after resolution; implementation is smaller but hover and errors lose
the vocabulary users chose.

## C. First-class callables in the owned-capture scope

### C1. Type spelling and storage boundary — Q13

**Recommend keeping thin `def` values and adding owned `Callable` storage**:

```text
type Unary = def(value: int64) -> int64
type Tool = Callable[def(value: int64) -> int64]
type Updater = Callable[mut def(value: int64) -> int64]
type Finish = Callable[own def() -> str]

# The outer parameter capability and inner call kind are different:
def apply(update: mut Updater, value: int64) -> int64:
    return update(value)
```

Inside `Callable[...]`, bare `def` denotes Shared, `mut def` Mutable, and
`own def` Consuming. Outside that storage constructor, a thin `def` is a
capture-free Shared function pointer; `mut def`/`own def` are not independent
thin types. Ordinary parameter `mut`/`own` still governs access to the value.
The inner parameter contracts independently carry bare/`mut`/`own` modes.

A named function or capture-free lambda stays Copy and Transfer with no
environment allocation. A concrete inferred capturing closure stays affine
and can retain its statically known environment type. Erasure occurs only
through an explicit constructor such as `Tool(lambda value: value + offset)`;
the surrounding constructor supplies the lambda's expected parameter types.
`Tool(existing_callable)` moves an already packed value/adapts its permitted
contract; it does not clone or add a second box. Parameters, results, fields,
and collection elements can carry that same `Tool` type across modules.

The alternative extends `def(...)` itself to all stored environments and adds
call-kind modifiers. It gives shorter signatures, but a general stored `def`
must become non-Copy and use an erased representation even for function-only
collections. Keeping a secretly Copy special case based on the current target
would make declared type identity insufficient for ownership checking. Both
alternatives can use the storage policy in C3; neither implies mandatory
boxing. The recommendation preserves the existing cheap, Copy function-value
category at the cost of two explicit storage categories.

Other keyword-free spellings considered: three type names
`SharedCallable`/`MutableCallable`/`ConsumingCallable` (more names for one
model), and mode parameters such as `Callable[Signature, Mutable]` (new
marker names beside the existing capability vocabulary). A reserved
`closure`, `fnmut`, or `fnonce` keyword is unnecessary. Python's
`Callable[[T], R]` shape alone cannot expose Aura's parameter capabilities,
names/defaults, or keyword-only restrictions, so the nested declaration-shaped
contract is an intentional departure.

### C2. Call kinds, capture ownership, and local mutation — Q14

**Recommend body-derived call kind with explicit weakening adapters**. Shared
reads captures and may repeat; Mutable mutates owned capture storage under
exclusive callable access; Consuming moves any non-Copy captured value and
consumes the callable on invocation. Consuming takes precedence if a body
both mutates and consumes. A Shared value can be intentionally restricted to
Mutable or Consuming, and Mutable to Consuming, through the destination
callable constructor. The reverse conversions are invalid. Ordinary
assignment does not silently change kind. The alternative requires exact
kind everywhere and a source wrapper for any restriction.

Ordinary lambdas retain by-value capture: Copy snapshots, non-Copy owners
move, and implicit capture of an enclosing shared/mutable parameter capability
fails. Copying such an input into an owned local first remains available.
Owned capture mutation is a proposed extension to ADR-0037's read-only
environment: a body may call a `mut` operation on its **owned** capture,
making the closure Mutable. Rebinding or mutating the outer variable is not
implied. A mutable callable local is required for a Mutable invocation even
when its source capture came from an immutable binding. Mutable parameters
of the lambda do not by themselves make its environment Mutable.

An explicit `[own state]` capture is eligible owned storage too; `[state]`
and `[mut state]` retain ADR-0038's shared/mutable **loan** meanings and cannot
be packed. No spelling is repurposed from loan to ownership. Capture-free
lambdas remain Shared, even if they mutate a supplied `mut` argument.

Shared invocation retains shared callable access through supplied arguments,
defaults, and the body; Mutable retains exclusivity over that interval and is
non-reentrant. A Consuming call transfers its right before evaluating ordinary
arguments, as selection of an owning receiver does: if an argument/default
traps, owned temporaries and the callable are destroyed and the right is not
restored. Never-called environments are destroyed normally. Capturing and
erased callable values are non-Copy and non-cloneable even when every capture
is Copy; `dict.get`, snapshots, or generic cloning cannot duplicate them.

### C3. Inline versus allocated environments — Q15

**Recommend a small inline buffer with checked overflow allocation**. On a
64-bit target an erased value is four words: one immutable operations-table
pointer plus three words (24 bytes) of inline storage, aligned to one word.
On another pointer width the same word-count rule applies. An environment
fits inline only if its canonical size and alignment fit that buffer; larger
or over-aligned environments use one owned allocation with the pointer in
the buffer. The table records which representation to access. Zero-capture
packing does not allocate. Moves of an existing packed value do not allocate.
Existing strings, lists, indirect fields, or resources retain their own
allocation/ownership behavior; “inline” does not mean their payloads become
allocation-free.

Overflow allocation uses a checked layout and fallible reserve/allocation;
controlled failure is `AU4005: cannot allocate callable environment` at
packing, never `Result.Err` or a null callable. No partly initialized value
is published. When packing a lambda directly, reserve its outer environment
before acquiring captures; acquire captures in lexical first-use order, or
explicit capture-list order. A failure during nested construction releases
the initialized prefix in reverse order. Packing an existing affine value
already selected as an owned argument does not restore that value after a
trap. The process-abort boundary of catastrophic host failure is not changed.

The number of words is a proposed initial ABI policy, not a measured optimum.
An optimizer may elide allocation only when all observable effects, including
the specified controlled-failure/cleanup behavior, are preserved. Benchmark
allocation counts, environment size, call overhead, and binary size separately.

| Storage alternative | Representation and allocation | Failure, destruction, and Transfer |
| --- | --- | --- |
| Small buffer plus overflow (recommended) | Four-word value; zero new allocations for fitting environments, one for larger ones | Checked overflow trap; dynamic table-selected drop of the active environment; C8's compiler-verified Transfer boundary |
| Fixed inline storage with explicit capacity in the type | `Callable[contract, capacity]`, tag/table plus caller-chosen bytes; oversized packing is rejected, never implicitly allocated | No environment allocation failure; same drop table; same structural Transfer validation; library signatures gain a capacity parameter and callers must manage large environments separately |
| Closed family of statically enumerated environments | Tag plus maximum environment payload, direct switch for call/drop; no environment allocation | No environment allocation failure; Transfer iff every enumerated environment qualifies; changing the family changes its type/ABI and cannot represent arbitrary imported open factories behind one contract |
| Always allocated | Two-word pointer/table descriptor and one allocation per nonempty environment | Checked allocation failure, table-selected drop plus free; same Transfer admission rule, but violates the no-mandatory-boxing guardrail and is not a ratifiable candidate |

All admitted alternatives are deterministic owners without garbage collection
or reference-counted sharing of a mutable environment. The closed-family
alternative requires public environment families or repeated specialization
through callers; it is not a free implementation of an open callable API.

### C4. Destruction and unavoidable erased dispatch — Q16

**Recommend one shared immutable operations table per concrete packing adapter**.
It contains invocation/default-binding entrypoints, the environment drop
entrypoint, size/alignment, and checked shape identity. Per-value cost is
one word for the table pointer, **not an additional standalone destructor
pointer**, but destruction still makes an indirect call through that table.
This is runtime dispatch, and the proposal does not claim otherwise. A
one-pointer table shares these entries among values of the same shape.

The alternative stores invocation and destructor pointers directly in every
value, adding at least one word beyond the recommended four-word layout
(further default-binding metadata is still needed). It saves a table lookup
but increases field/collection footprint. With an open erased contract, a
single static drop function cannot know arbitrary imported environments.
Avoiding all indirect destruction requires the closed-family restriction in
C3, or equivalent specialization, not just an optimizer promise.

Implicit captures initialize in lexical first-use order; explicit captures
initialize in written order. Destroy captures in reverse initialization order,
recursing through each capture's ordinary drop rules. Invocation does not
destroy a repeatable environment; successful consuming invocation transfers
the values it consumes and drops the remaining captures once. A packed bound
receiver is one capture. Do not call an arbitrary `close` method just because
it exists: resource cleanup remains governed by the established resource/
`with` rules, and opaque foreign wrappers have no automatic foreign close.

The operations table does not perform unrestricted trait lookup or contain
a user-replaceable destructor address. Checked MIR/layout metadata selects
generated adapters; source has no table/address introspection. Both backends
use the same capture/drop plan and source diagnostic identity. A callee/default
trap retains the public target and source span, not synthetic thunk frames.
Cancellation and forced reset preserve the A9 boundary.

### C5. Heterogeneous values, names, restrictions, and variance — Q17

**Recommend exact complete contracts, with explicit safe restrictions**. All
packed values of `Tool` have the same normalized call kind, parameter types
and modes, parameter names or explicit positional-only slots, keyword-only
boundary, default-availability bits, result/origin contract, and inferred
clone/equality obligations. Capture sets may differ; representation erases
their shapes, not their operational rights.

Named slots use `name: [mut|own] Type`; unnamed `Type`, `mut Type`, and
`own Type` slots expose positional calls only. Named and unnamed slots may
mix before `*`; every slot after `*` must be named. There is at most one
`*`, it must be followed by a parameter, and it adds no variadics. A trailing
comma remains deferred to Batch 4. This grammar applies to thin contracts and
the contracts nested inside `Callable`/`TaskCallable`.

The destination constructor may remove exposed names/default availability,
or restrict a positional-or-keyword slot to keyword-only, when every call
permitted by the destination is valid for the source. It cannot turn a
keyword-only source slot into a positional slot, rename a keyword, invent
a default, change an argument capability/type, or strengthen a result
guarantee. Renaming requires an explicit source wrapper. Parameter and result
types are invariant; there is no general callable subtyping. Call-kind
restrictions follow C2. A thin callable alias may be used as an explicit
contract adapter, for example `Unary(function)`; no environment is added for
a capture-free target. A bare assignment with a differing complete contract
is an error, rather than the current silent metadata erasure.

```text
type Tool = Callable[def(value: int64) -> int64]

def choose(flag: bool, left: own Tool, right: own Tool) -> Tool:
    return left if flag else right

tools: list[Tool] = [Tool(first_closure), Tool(second_closure)]
```

An explicit common callable type resolves ADR-0037's multi-branch rejection.
Each reached branch packs exactly one eligible environment; untaken lambda
expressions acquire no captures. Existing branch move merging remains
conservative: if a variable is moved on one reachable path, a later use must
prove it initialized on all paths. Branches cannot infer an erased common
contract merely because their ABI shapes match. Without an explicit common
storage contract, different capturing closures still fail to merge.

The alternative admits any safe restriction implicitly at assignment and
flow joins. It is convenient but makes the visible contract depend on where
inference loses information, the problem ADR-0058 is meant to remove.

### C6. Bound methods on owned or Copy receivers — Q18

**Recommend ordinary `receiver.method` value selection with by-value receiver
capture**. Evaluate the receiver once. An owned non-Copy receiver moves as a
whole into the binding; a Copy receiver is snapshotted. A fresh owned temporary
is eligible. A shared/mutable non-Copy receiver or view cannot be stored by
this spelling; explicitly clone to an owned local when supported, or keep
an ADR-0038 loan closure local. Copy reads through an eligible capability may
materialize the ordinary independent snapshot, never a stored view descriptor.

The selected method's receiver determines the callable kind: shared `self`
is Shared, `mut self` is Mutable, and `own self` is Consuming, **including
for a Copy receiver**. A `mut self` binding mutates its captured receiver,
not the original outer place; it requires a mutable callable place to invoke.
Dropping it drops the owned receiver under C4. Defaults and names exclude
the now-bound receiver slot and preserve the ordinary parameter contract.
The method selection is statically resolved, including a unique applicable
trait method; generic method arguments must be concrete from specialization
or the expected contract. Associated methods without a receiver become thin
function values by the same static resolution, without a hidden environment.
Extern declarations remain direct-call-only, and there is no unbound
instance-method descriptor protocol.

The alternative requires an explicit `receiver.bind(method=Type.method)`
operation. It advertises capture more clearly but adds a method-selector
surface and builtin operation. The recommendation matches Python's selection
shape while following Aura's existing move-at-capture semantics. A diagnostic
on later receiver use points back to method binding as the move origin.

### C7. Defaults, keyword-only declarations, and forwarding — Q19 and Q20

**Q19 recommends availability in type identity and target-owned default
evaluation**. A callable type writes `= ...` for an available default; only a
declaration supplies its expression. A literal or arbitrary expression after
`=` in a callable *type* is rejected. `...` here is a default-availability
marker, not a value, variadic signature, or parameter pack.

```text
def render(value: int64, *, prefix: str = "value") -> str:
    return f"{prefix}: {value}"

type Renderer = Callable[def(value: int64, *, prefix: str = ...) -> str]
renderer = Renderer(render)
print(renderer(value=4))
```

Function, inherent method, trait, implementation, and contextual lambda
parameter lists gain the keyword-only `*` boundary. Lambdas still have no
inline types, defaults, generic parameters, or statement bodies; their body
names must match exposed names in the expected contract, and their `*`
boundary must match. A lambda cannot satisfy a contract promising defaults
without an ordinary named callable that declares them. Trait/impl methods still cannot
declare ordinary defaults. Trait-bound callers use the trait's public names
and keyword restrictions; implementation-local parameter names may differ
by ordinal as today. Conformance adds keyword-only comparison, and concrete
trait-method values expose the selected trait contract rather than making
its implementer's local names a second interface.

Required parameters may follow a defaulted parameter only across the `*`
boundary or within the keyword-only group; positional parameters retain the
existing required-before-default order. `mut` parameters and returned-view
origin parameters cannot default. Default expressions cannot refer to the
same declaration's parameters/receiver. They run in the defining module's
context, not the importing caller's namespace.

Select/retain the callable first, evaluate supplied expressions in written
source order, capture their values/access once, then evaluate omitted defaults
in declaration-slot order, and invoke the body. A supplied slot suppresses
its default. The selected target's default code is used even after a return,
field load, collection move, or module import. Default *expression identity*
is an implementation reference in the checked interface and complete cache
hash, not structural callable type identity: two functions with different
default expressions can share a contract that promises the same defaulted
slots. A thin value can be one pointer to an adapter entrypoint that accepts
the supplied-slot bitmap and fills defaults; it need not gain a per-value
default pointer. Erased values use C4's table.

The alternative puts default-expression identity into callable type equality.
It preserves behavior but prevents unrelated functions with equivalent
calling contracts from sharing a collection or factory result type.

Ordinary forwarding of a callable value preserves its descriptor and default
references. A handwritten wrapper that supplies every argument causes no
inner default evaluation. A wrapper that intentionally omits inner slots
invokes that target's defaults there; this is an ordinary new call, not a
claim of transparent wrapping.

**Ratified Q20: B — no wrapper helper.** Batch 1 adds no `base.wrap(body)`
intrinsic or signature-copying operation. Handwritten wrappers are ordinary
functions, methods, or closures with their own declared or expected contract.
Names, keyword-only restrictions, and available defaults belong to that
wrapper's contract; they are not automatically inherited from a selected
inner callable. A lambda still cannot promise defaults of its own.

```text
def render_with_prefix(base: Renderer, value: int64, *, prefix: str = "wrapped") -> str:
    return base(value, prefix=prefix)

type RequiredRenderer = Callable[def(value: int64) -> str]

def use_target_default(base: own Renderer) -> RequiredRenderer:
    return RequiredRenderer(lambda value: base(value))
```

The first wrapper evaluates its own omitted prefix before entering its body
and supplies every inner argument, so the inner prefix default does not run.
The second owns the selected base and exposes an all-required outer contract;
each invocation enters the lambda body and then evaluates that base's prefix
default as part of the inner call. There is no synthesized default forwarding
before wrapper entry. If a wrapper deliberately omits a slot at several inner
calls, that target's default runs freshly for each call under ADR-0015.

Ordinary capture, call-kind, destruction, and argument-origin view rules apply
without wrapper-specific metadata or origin remapping. Replaying an owned
argument still requires an explicit permitted reconstruction/clone. A wrapper
can enter TaskCallable only through C8's normal structural admission rules;
capturing an ordinary erased Callable cannot recover Transfer evidence.
Defaults and names do not become runtime-reflectable objects. The cost of Q20
B is that a reusable wrapper cannot automatically inherit the full default
policy of a runtime-selected target. General signature polymorphism and
decorator registration remain Batch 6; the unselected Q20 A adapter is absent
from this implementation plan.

### C8. Structural Transfer and task targets — Q21

Erasure must not discard the evidence required by
[ADR-0033](decisions/0033-structural-transfer-and-task-results.md).
**Recommend `TaskCallable[contract]` as a checked storage constructor** in
addition to task-local `Callable[contract]`. It has the same representation
and call-kind grammar; it admits only concrete environments whose complete
owned capture tree is structurally Transfer. `random.Rng`, live host
resources, views, and loan closures fail at construction with `AU3008`, naming
the capture path. A user trait named `Transfer` confers no permission. This
is a compiler-verified restriction on what the storage may contain, not a
source assertion overriding the derivation.

An ordinary erased `Callable` is conservatively non-Transfer, including when
its current hidden target happens to qualify. A concrete inferred closure or
thin function still uses its directly known structural classification.
`TaskCallable` can explicitly weaken to `Callable`; the reverse cannot
recover erased evidence through a cast or runtime test. An unresolved generic
capture at this boundary is rejected, retaining ADR-0033's no-inferred-Transfer-
obligation baseline. All aggregates containing these types follow ordinary
structural rules. `TaskCallable` remains non-Copy and non-cloneable; a task
result containing one keeps a single-consumer observation right.

All four TaskGroup start methods accept an eligible stored target by move
for one child invocation, whether Shared, Mutable, or Consuming. Mutable
means exclusive access to the **child-owned callable environment**, so no
parent writeback is implied. Each explicit/default argument is evaluated in
the parent start operation under ADR-0015, then copied/moved into child-owned
capture storage. Bare target parameters share that storage; `own` parameters
consume it. A target with any ordinary `mut` parameter remains rejected.
Every captured argument and the result must be concretely Transfer, even for
`start_soon`. Validate the full boundary before scheduling; default failures
start no child. The target/default source frames and spawn ancestry survive
the adapter, and existing result-observation rules remain unchanged.

The alternative puts a compiler-owned Transfer-bound slot on `Callable`,
for example `Callable[contract, Transfer]`. It uses one type constructor but
needs a contextual meaning distinguishable from the already legal user trait
of that name. Both designs must prove admission statically; neither permits
unchecked assertions or dynamic task acceptance.

### C9. Equality and returned-view contracts — Q22

Callable equality is **not an open choice**. Under
[ADR-0044](decisions/0044-canonical-collection-surface.md), functions, closures,
bound methods, and aggregates containing them lack `==`, `!=`, hash/key, and
equality-dependent collection operations. No function address, environment
address, capture equality, or method/receiver identity supplies a hidden
equality relation. Rendering uses an opaque callable label without an address;
users who need registry identity store explicit keys.

**Recommend stored callables may return a view of one explicit ordinary
argument**, with `-> view [mut] T from name` in their complete contract.
An origin is encoded by ordinal and retains ADR-0038's footprint, source-place,
default, ownership, containment, and handoff requirements. This stores code
that will create a view when invoked; it does not store a view or a loan
capture. A bound method returning a view `from self` cannot be packed in
Batch 1: its receiver has moved into hidden storage and would need a
callable-self origin/lifetime contract. A result from another explicit
parameter is eligible. Task targets cannot return views.

The alternative defers every view-returning stored callable alongside loan
captures, leaving these functions direct-call-only. It reduces the first
matrix but defers an expressible parameter-origin contract that ADR-0058
already requires preservation of when exposed.

### C10. Loan storage remains deferred

No packed callable may contain `[owner]`, `[mut owner]`, a view descriptor,
or an aggregate that hides one. Existing immediate/local and compiler-known
synchronous non-retaining callbacks retain their ADR-0038 behavior. They must
not be forced through an owned `Callable` constructor and thereby lose loan
provenance. Builtin callback lowering can share the new invocation plan while
retaining a distinct statically non-retaining admission path.

Existing Shared callback sites, including list/Array algorithms and
`control.retry`, also admit packed Shared Callable/TaskCallable values with
their required arity, parameter modes/types, and result contract. They borrow
the value for the operation; they neither clone it nor erase its contract.
Their actual positional call pattern must be permitted, so a keyword-only
element parameter cannot masquerade as a positional callback. Existing
concrete/loan callback acceptance uses the same check without packing.
Mutable and Consuming callbacks remain ineligible at these Shared sites;
this phase adds no mutable standard-library algorithm callback contract.

The joint Batch 1–2 design owns capture origins, aggregate lifetime propagation,
receiver-loan binding/reborrow timing, indexed invalidation, and callback-self
returned views. Until then users must move/clone an owner into a stored
callable, pass explicit state per invocation, or keep a loan closure local.
They cannot retain a callback over a borrowed session or collection entry in
a field/registry, even when the runtime happens to keep that storage alive.

## D. Option removal and complete library audit

### D1. Audit provenance and counted surfaces

The supplied planning inventory is **128 occurrences across 28 Manual pages,
70 fixtures, 26 tutorial hits, and 151 compiler-source hits**. It has no
attached search expression or source revision. Preserve it as planning input,
not a claim that these incomparable units describe the current tree exactly.
At the source baseline above, a read-only `\bOption\b` scan found:

| Scope and file selection | Occurrences | Matching lines | Files |
| --- | ---: | ---: | ---: |
| `docs/manual/**/*.md` | 151 | 140 | 21 |
| `tutorials/**/*.md` | 78 | 72 | 13 |
| `crates/aura-compiler/tests/fixtures/**/*.au` | 189 | 173 | 54 |
| `crates/aura-compiler/src/**/*.rs`, including Rust Option and tests | 2,146 | 2,087 | 36 |

Rust's own `Option<T>` is not being removed. A narrower quoted-name scan for
`"Option"` found 443 occurrences in 20 compiler source files, but still
includes tests and is not an API count. AST `?`, constructors, generated docs,
CLI fixtures, examples, LSP metadata, and runtime option helpers need separate
inventory even when they contain no bare `Option` word. The
[API index](../docs/manual/api-index.md) has 40 Option-bearing rows: **39
public functions/methods plus the builtin Option enum row**. The table below
accounts for all 39 and was cross-checked against
[`builtin_modules.rs`](../crates/aura-compiler/src/builtin_modules.rs),
[`call.rs`](../crates/aura-compiler/src/call.rs), and
[`sema.rs`](../crates/aura-compiler/src/sema.rs).

### D2. Absent versus present None — Q23

**Ratified Q23: A — two operation-specific nominal outcome families**, using the
existing enum mechanism and qualified constructors/patterns:

```text
enum Lookup[T]:
    Found(value: T)
    Missing

enum Poll[T]:
    Ready(value: T)
    Unavailable
```

These are compiler-provided generic data enums for lookup/poll outcomes, not
a second optional type spelling or an alias for `T | None`. Their payloads
are owned; Copy, clone, equality, and Transfer follow the all-payload builtin
data-enum rules after substitution. `Lookup.Found(None)` differs from
`Lookup.Missing`, and `Poll.Ready(None)` differs from `Poll.Unavailable`.
Unqualified constructors follow the existing unambiguous expected-enum rule;
public examples use qualified names.

All generic lookup APIs use the tagged result even for non-optional
specializations. Their declared return type never changes by specializing T,
and a generic caller never loses information. `Queue.get_or_none` and
`Task.result_or_none` are replaced by `poll`, returning `Poll[T]` for the
same immediate/default-timeout and collapsed-unavailable policy. Full-detail
`Queue.get`/`Task.result` continue returning their existing nominal outcomes.
The alternative gives each operation its own nominal result type, such as
`ListLookup`, `DictLookup`, `QueuePoll`, and `TaskPoll`; it offers more domain
specificity but multiplies identical case-handling code. Neither alternative
flattens a possibly optional payload or renames a generic Option shim.

Only five **public** APIs need these tags today: `list.get`, `dict.get`,
`dict.remove`, `Queue.get_or_none`, and `Task.result_or_none`. The private
dictionary replacement operation also needs the previous-value distinction.
An `Array[T]` scalar cannot be None, and `json.Value.Null` is already nominally
distinct from Aura None. The future iterator protocol likewise needs tagged
item/end outcomes, but its public names remain owned by Batch 9 under the
roadmap; this checkpoint does not ratify those names ahead of that batch.

**Required Batch 2 follow-up:** the element-loan design revisits an app-facing
`V | None` form of `dict.get`. Review its access capability and result lifetime
together with dictionary-entry loans, and state how callers distinguish a
missing entry from a present None when V is itself optional. Batch 1 retains
`dict.get -> Lookup[V]` uniformly, including its clone-safe observation rule;
the follow-up neither changes that accepted replacement now nor silently
flattens the information-preserving generic result. Batch 2 must make its
app-facing choice explicit alongside any retained tagged lookup surface.

### D3. Every public Option signature and its replacement

In this table, `B` abbreviates `list[uint8]` in prose only. Parameters not
listed are unchanged, including ownership, timeout defaults, input bounds,
error variants, and resource cleanup. `Result` still distinguishes failure
from a successful absent result; empty strings/bytes/datagrams remain present.

| # | Current API / Option position | Proposed replacement | Preserved distinction or obligation |
| ---: | --- | --- | --- |
| 1 | `str.strip_prefix(text: str) -> Option[str]` | `-> str \| None` | Matched empty remainder differs from no match |
| 2 | `str.strip_suffix(text: str) -> Option[str]` | `-> str \| None` | Same suffix rule |
| 3 | `Array.get(index: list[int64]) -> Option[T]` | `-> T \| None` | Numeric payload or out-of-bounds absence; rank checks unchanged |
| 4 | `Array.set(index: list[int64], value: T) -> Option[T]` | `-> T \| None` | A valid replacement returns its old scalar; invalid coordinate/rank still traps, rather than becoming None |
| 5 | `list.get(index: int64) -> Option[T]` | `-> Lookup[T]` | `Found(value)` / `Missing`; retain negative normalization and clone-safe observation |
| 6 | `dict.get(key: K) -> Option[V]` | `-> Lookup[V]` | `Found(value)` / `Missing`; clone-safe V, including a present None |
| 7 | `dict.remove(key: K) -> Option[V]` | `-> Lookup[V]` | Move removed value into `Found`, or `Missing`; no clone requirement |
| 8 | `Queue.get_or_none(timeout: Duration = ...) -> Option[T]` | `poll(timeout: Duration = ...) -> Poll[T]` | `Ready(item)` / `Unavailable`; closed, timeout, cancellation, and immediate absence stay collapsed |
| 9 | `Task.result_or_none(timeout: Duration = ...) -> Option[T]` | `poll(timeout: Duration = ...) -> Poll[T]` | `Ready(value)` / `Unavailable`; every attempt still consumes a non-repeatable observation right |
| 10 | `io.read_line() -> Result[Option[str], io.Error]` | `-> Result[str \| None, io.Error]` | None only on clean EOF |
| 11 | `sys.env(name: str) -> Option[str]` | `-> str \| None` | Missing variable versus present empty text |
| 12 | `path.parent(path: str) -> Option[str]` | `-> str \| None` | Absent path component |
| 13 | `path.file_name(path: str) -> Option[str]` | `-> str \| None` | Absent path component |
| 14 | `path.extension(path: str) -> Option[str]` | `-> str \| None` | Absent extension versus a present result |
| 15 | `json.dumps(..., indent: Option[int64] = None)` | `indent: int64 \| None = None` | None keeps compact output; numeric validation unchanged |
| 16 | `json.as_bool(value: json.Value) -> Option[bool]` | `-> bool \| None` | Bool variant only |
| 17 | `json.as_int(value: json.Value) -> Option[int64]` | `-> int64 \| None` | Int variant only; no Float conversion |
| 18 | `json.as_float(value: json.Value) -> Option[float64]` | `-> float64 \| None` | Float variant only; no Int conversion |
| 19 | `json.into_string(value: own json.Value) -> Option[str]` | `-> str \| None` | Consume input; wrong variant yields None |
| 20 | `json.into_array(value: own json.Value) -> Option[list[json.Value]]` | `-> list[json.Value] \| None` | Consume input; empty array remains present |
| 21 | `json.into_object(value: own json.Value) -> Option[dict[str, json.Value]]` | `-> dict[str, json.Value] \| None` | Consume input; empty object remains present |
| 22 | `net.TcpStream.read_line(...) -> Result[Option[str], io.Error]` | `-> Result[str \| None, io.Error]` | None on EOF; empty line is present |
| 23 | `net.TcpStream.read_bytes(...) -> Result[Option[B], io.Error]` | `-> Result[B \| None, io.Error]` | EOF versus bytes; byte-count rules retained |
| 24 | `net.UdpSocket.recv(...) -> Result[Option[B], io.Error]` | `-> Result[B \| None, io.Error]` | None on deadline; empty datagram is present |
| 25 | `net.UdpSocket.recv_from(...) -> Result[Option[net.UdpDatagram], io.Error]` | `-> Result[net.UdpDatagram \| None, io.Error]` | Deadline versus owned datagram/address |
| 26 | `net.WebSocket.recv_text(...) -> Result[Option[str], io.Error]` | `-> Result[str \| None, io.Error]` | None on close; empty message is present |
| 27 | `net.WebSocket.recv_bytes(...) -> Result[Option[B], io.Error]` | `-> Result[B \| None, io.Error]` | Same message/close distinction |
| 28 | `net.UnixStream.read_line(...) -> Result[Option[str], io.Error]` | `-> Result[str \| None, io.Error]` | EOF versus line |
| 29 | `net.TlsStream.read_line(...) -> Result[Option[str], io.Error]` | `-> Result[str \| None, io.Error]` | EOF versus line |
| 30 | `process.start(..., cwd: Option[str] = None, ...)` | `cwd: str \| None = None` | None inherits working directory; shared parameter mode retained |
| 31 | `process.run(..., cwd: Option[str] = None, ...)` | `cwd: str \| None = None` | Same working-directory policy |
| 32 | `process.Child.stdin() -> Option[process.Pipe]` | `-> process.Pipe \| None` | Configured/available pipe versus absence; resource ownership retained |
| 33 | `process.Child.stdout() -> Option[process.Pipe]` | `-> process.Pipe \| None` | Same pipe policy |
| 34 | `process.Child.stderr() -> Option[process.Pipe]` | `-> process.Pipe \| None` | Same pipe policy |
| 35 | `process.Child.wait_or_none(...) -> Result[Option[process.ExitStatus], process.Error]` | `-> Result[process.ExitStatus \| None, process.Error]` | Status / timeout / error remain distinct |
| 36 | `process.Pipe.read_line(...) -> Result[Option[str], process.Error]` | `-> Result[str \| None, process.Error]` | None only on EOF |
| 37 | `process.Pipe.read_bytes(...) -> Result[Option[B], process.Error]` | `-> Result[B \| None, process.Error]` | EOF versus bytes |
| 38 | `process.Supervisor.start(..., cwd: own Option[str] = ..., ...)` | `cwd: own (str \| None) = None` | Own retained restart configuration; no hidden clone |
| 39 | `process.Supervisor.wait_or_none(...) -> Result[Option[process.SupervisorEvent], process.Error]` | `-> Result[process.SupervisorEvent \| None, process.Error]` | Event / timeout / error remain distinct |

`BuiltinMember::MapSet` also returns an internal `Option[V]` today. It is
resolved only by `resolve_runtime`, not public source member lookup. Change
its checked previous-value result to `Lookup[V]` and drop any ignored previous
owner correctly; **do not expose `dict.set`**. `list.pop` already returns T
and traps on invalid removal; it is not an Option API. `Queue.get_or` and
`Task.result_or` already return T with an owned fallback and remain unchanged.
No current user-callable iterator `next` API is missing from this table.

### D4. Exact order and clean-slate removal

1. Land phase 1's unions, aliases, conditional narrowing, generic-member rules,
   nullable-FFI adapter, owned callables, and bound methods on both backends.
   Keep the existing Option library intact at the intermediate reviewable
   checkpoint. This is development staging, not a published compatibility mode.
2. Freeze that checkpoint's usability diff and semantic tests. Prove all
   [roadmap removal criteria](14-priority-roadmap.md#priority-batches), including
   stable-place narrowing, `V | None` specialization, and the stated FFI rule.
   Do not publish a release presenting both optional spellings as permanent.
3. Add failing phase 2 tests for **every table row**, nested optional payloads,
   ownership, and ordinary unknown-name/parse errors. Then convert every
   public and private library signature/result constructor to the chosen union
   or tagged case. Update builtin metadata, semantic analysis, MIR, native
   adapters, and imported interface serialization together.
4. Remove the language builtin Option type, `Option.Some`/`Option.None`, their
   contextual short constructors, `T?` desugaring, and both renamed generic
   `*_or_none` members. A user may still declare an ordinary nominal type
   named `Option`; it receives no builtin behavior. Rust host Option remains.
5. Update fixtures, tutorials, Manual, examples, and generated documents in
   that order within the **same change family**, together with CLI/LSP and
   extension expectations. These are dependency steps, not independently
   shippable partial conversions. Existing ADR/work records remain as records.
6. Bump semantic database/interface, serialized MIR, native cache, analysis/LSP,
   and generated-reference identities where their formats/meaning change;
   regenerate interface and fence hashes from the reviewed new sources.
   Run the full acceptance matrix before declaring removal complete.

After removal, absent a user declaration, `Option[int64]` produces the ordinary
**AU2001**, with message ``unknown type `Option` `` at the type name. The `T?` type suffix then
receives an ordinary `AU1101` unexpected-token error; an absent constructor/member
receives an ordinary name/member error. There is no optional-specific parser
branch for Option, migration diagnostic, compatibility alias, quick fix,
spelling reservation, or user migration tool. Negative tests assert the
ordinary rule using arbitrary unknown names as well as this example.

## E. Structural split of sema.rs

### E1. Proposed boundaries and current source anchors

The baseline [`sema.rs`](../crates/aura-compiler/src/sema.rs) is 29,693 lines.
It already combines the checked program, `Type`, callable metadata, structural
properties, name resolution, expression checking, and place/loan dataflow.
The extraction must preserve public types/re-exports and serialized meanings
before new fields change the schemas. These are engineering choices, not
questionnaire items.

| Proposed module | Baseline responsibilities to extract | Batch 1 additions |
| --- | --- | --- |
| `sema.rs` facade and `sema/program.rs` | `Program`, namespaces, declaration inventory and check orchestration, currently near lines 28–416 and `check_with_context` | Register aliases/callable type constructors; retain external checking entrypoints |
| `sema/types.rs` | `Type`/equality/display, `lower_type`, substitution/unification, currently around 579–741, 3323–3581, 4860–5384 | Canonical union keys, singleton normalization, symbolic member/remapping rules, alias expansion |
| `sema/properties.rs` | Copy/clone, `transfer_shape`, equality eligibility, task-observation derivation | All-member union properties, erased callable non-cloneability and checked TaskCallable admission |
| `sema/capabilities.rs` | Declaration-stable passing modes, move/owned/shared/mutable checks, argument acquisition | Callable kind restriction, affine invocation, narrowed access without capability escalation |
| `sema/places.rs` | `PlaceProjection`, `ProjectionPath`, `PlacePath`, overlap and generation-facing identity, currently around 5804–5918 | Canonical union payload projections and tag locks; no indexed/keyed place extension |
| `sema/loans.rs` | `ViewBinding`, active match loans, return-origin footprints, last-use/reborrow/escape analysis | Containment of narrowed payload access, stored argument-origin view results, explicit loan-pack rejection |
| `sema/flow.rs` | Local initialization/move state, branch joins, loop flow and expression-result state | Member-set facts, invalidation, short-circuit conditions, distinct return/break/continue successors |
| `sema/callables.rs` | `FunctionParamContract`, `ClosureInfo`, lambda typing, callable merging and metadata erasure | Complete contract identity, owned packing, bound receiver plans, default/keyword metadata, ordinary forwarding |
| `sema/patterns.rs` | Pattern checking, match capabilities, guard and exhaustiveness logic | Type patterns, union coverage, generic overlap rejection and singleton pattern normalization |
| `sema/resolve.rs` and `sema/traits.rs` | Imports, visible types/functions, trait applicability/conformance and inferred obligations | Alias visibility, complete callable contract conformance, all-member nominal trait dispatch |
| `sema/expressions.rs` and `sema/builtins.rs` | `FunctionChecker` expression visitors and builtin result typing | Expected-union propagation, FFI adapter eligibility, all audited library result types |

Do not create one module per syntax token. Split along data ownership and
dependency direction: type identities are independent of body flow; places
describe paths; loans/capabilities consume paths and property queries; body
checking coordinates them. `LocalBinding` should contain references/records
owned by these analyses rather than copies of parallel flags whose meanings
diverge. Preserve a thin facade during extraction so downstream `mir`,
`analysis`, `package`, and CLI callers do not move at the same time as logic.

### E2. Extraction order and gates

1. Characterize current observable contracts and exported serialization; move
   type/program definitions and pure property helpers first. Preserve Type's
   current equality and schema before adding union/callable fields. Existing
   fixtures should remain byte-identical through this structural step.
2. Extract place identity/overlap, then loan provenance and return footprints.
   Pin disjoint fields, same-root alternative returned views, reborrow
   suspension, branch expiry, and existing cancellation boundaries before
   relocating their logic. Do not convert loans into cloned runtime values.
3. Extract capability checks and callable resolution/default binding. Keep the
   existing lambda/storage failures until the new tests for that feature are
   introduced. Move trait/name lookup helpers only as their dependencies become
   explicit; avoid simultaneous semantic changes in an extraction patch.
4. Introduce a flow-state abstraction with distinct exit kinds. Preserve
   existing move checking, then add failing Boolean/narrowing fixtures before
   implementing their transfer functions. Add unions/aliases in `types`,
   patterns in `patterns`, and ownership effects in `capabilities`/`loans`.
5. Add complete callable contracts in `callables`, storage derivation in
   `properties`, and resolved packing/invocation effects in shared MIR
   lowering. Phase 2 then updates centralized builtin signatures and removes
   the language Option branches without rewriting unrelated host Option use.
6. Leave a small coordination facade; verify that extracting `expressions`
   and `builtins` has not merely moved a second monolith or introduced a
   second copy of Type/PlaceId. No runtime/compiler crate separation is part
   of this split; that remains Batch 5.

Run relevant existing unit/fixture suites after each extraction; run the
complete forced MIR/direct matrix, affected CLI acceptance, and unchanged
compiler coverage floors at each completed semantic-boundary move. Production
code stays in ordinary module files covered by the current coverage regex;
do not put it in `_tests.rs`, broaden exclusions, lower the **96.30% lines /
97.21% functions / 94.71% regions** floors, or rewrite unrelated expectations
to accommodate a refactor. Newly specified behaviors get failing tests first;
the extraction's characterization tests assert semantics/interfaces, not line
placement. Where a new MIR contract is required, its contract test fails until
that descriptor/plan exists.

Follow [the ordered backend-boundary moves](15-backend-boundary.md#ordered-batch-1-moves-and-gates)
incrementally: typed operation descriptors, shared numeric/trap selection,
canonical place/loan plans, explicit ownership/exit actions, and then the
callable/default/receiver ABI plan. Keep a builder with only block, scalar,
load/store, resolved call/effect, and terminator emission responsibilities.
Union injection/tag/drop, callable packing/default order, and FFI conversion
selection are resolved before emission. The interpreter consumes the same
plans; Cranelift chooses machine instructions/registers, not language rules.
No backend switch, scheduler rewrite, runtime crate split, or frame-contract
weakening is smuggled into Batch 1.

Before/after heavyweight coverage/parity/full-CI work, inspect `target/` and
free disk per AGENTS.md; clean only obsolete build artifacts when its existing
thresholds require it. This document-only checkpoint requires none of those
heavy builds.

## F. Reference agent before and after

The version-0 [source](../examples/agents/tool_runner/src/main.au) has 131
lines and is pinned at `fc0361bc76b8b47d300a3089b1b4bb7c2b739dfc`.
It contains only capture-free functions in a repeated long registry type,
uses Option patterns around JSON extraction and dictionary observation,
and already has typed errors, retry, a Queue streaming loop, and cleanup.

The following is the complete 143-line candidate **after both phases**, under
all recommendations. It is intentionally not installed in `examples/` and has
not been compiled or run. `make_double` demonstrates an escaping owned capture;
`Increment(...).run` demonstrates an owned bound receiver; `value=` demonstrates
preserved registry-call names. The expected output changes only the version
header to `tool-runner version 1`; all subsequent lines should match
[version 0's stdout](../examples/agents/tool_runner/stdout.txt).

```text
import control
import json
import metrics


enum ToolError:
    InvalidRequest
    UnknownTool(str)
    NotReady


class ToolRequest:
    tool: str
    value: int64

    def to_json(self) -> json.Value:
        return json.Value.Object({"tool": json.Value.String(self.tool.clone()), "value": json.Value.Int(self.value)})

    def from_json(value: own json.Value) -> Result[ToolRequest, ToolError]:
        mut fields = json.into_object(value)
        if fields is None:
            return Result.Err(ToolError.InvalidRequest)
        if fields.len() != 2:
            return Result.Err(ToolError.InvalidRequest)
        match own fields.remove("tool"):
            case Lookup.Found(json.Value.String(tool)):
                match own fields.remove("value"):
                    case Lookup.Found(json.Value.Int(number)):
                        return Result.Ok(ToolRequest(tool=tool, value=number))
                    case _:
                        return Result.Err(ToolError.InvalidRequest)
            case _:
                return Result.Err(ToolError.InvalidRequest)


class ToolResult:
    tool: str
    value: int64

    def to_json(self) -> json.Value:
        return json.Value.Object({"tool": json.Value.String(self.tool.clone()), "value": json.Value.Int(self.value)})

    def from_json(value: own json.Value) -> Result[ToolResult, ToolError]:
        match own ToolRequest.from_json(value):
            case Result.Ok(request):
                return Result.Ok(ToolResult(tool=request.tool.clone(), value=request.value))
            case Result.Err(error):
                return Result.Err(error)


type Tool = Callable[def(value: int64) -> Result[ToolResult, ToolError]]


class Session:
    name: str

    def close(mut self) -> None:
        print(f"closed {self.name}")


def scaled(value: int64, factor: int64) -> Result[ToolResult, ToolError]:
    return Result.Ok(ToolResult(tool="double", value=value * factor))


def make_double(factor: int64) -> Tool:
    owned_factor = factor
    return Tool(lambda value: scaled(value, owned_factor))


class Increment:
    delta: int64

    def run(self, value: int64) -> Result[ToolResult, ToolError]:
        return Result.Ok(ToolResult(tool="increment", value=value + self.delta))


def dispatch(request: ToolRequest, registry: mut dict[str, Tool]) -> Result[ToolResult, ToolError]:
    match own registry.remove(request.tool):
        case Lookup.Found(tool):
            outcome = tool(value=request.value)
            registry[request.tool.clone()] = tool
            return outcome
        case Lookup.Missing:
            return Result.Err(ToolError.UnknownTool(request.tool.clone()))


def connect_tools() -> Result[int64, ToolError]:
    metrics.increment("tool-runner-attempts", 1)
    if metrics.get("tool-runner-attempts") < 3:
        return Result.Err(ToolError.NotReady)
    return Result.Ok(metrics.get("tool-runner-attempts"))


def stream_events(events: Queue[str]) -> None:
    events.put("stream started")
    events.put("token tools")
    events.put("token ready")
    events.put("stream finished")
    events.close()


def run_tools() -> Result[int32, ToolError]:
    mut registry: dict[str, Tool] = {"double": make_double(2), "increment": Tool(Increment(delta=1).run)}
    request = try ToolRequest.from_json(json.Value.Object({"tool": json.Value.String("double"), "value": json.Value.Int(21)}))
    print(f"request {json.dumps(request.to_json())}")
    result = try dispatch(request, registry)
    decoded = try ToolResult.from_json(result.to_json())
    print(f"result {json.dumps(decoded.to_json())}")
    next_request = ToolRequest(tool="increment", value=decoded.value)
    next_result = try dispatch(next_request, registry)
    print(f"result {json.dumps(next_result.to_json())}")

    match ToolRequest.from_json(json.Value.Null):
        case Result.Err(ToolError.InvalidRequest):
            print("rejected invalid request")
        case _:
            return Result.Err(ToolError.InvalidRequest)
    match dispatch(ToolRequest(tool="missing", value=0), registry):
        case Result.Err(ToolError.UnknownTool(name)):
            print(f"unknown tool {name}")
        case _:
            return Result.Err(ToolError.InvalidRequest)

    metrics.reset()
    attempts = try control.retry(connect_tools, max_attempts=3, initial_backoff=0ms)
    print(f"ready after {attempts} attempts")
    events = Queue[str]()
    with TaskGroup() as group:
        group.start(stream_events, events)
        for event in events:
            print(event)
    return Result.Ok(0)


def main() -> int32:
    print("tool-runner version 1")
    with session = Session(name="tool-runner"):
        match run_tools():
            case Result.Ok(code):
                return code
            case Result.Err(error):
                print(error)
                return 1
```

The registry annotation becomes one named contract, tools can own state beyond
their factory scope, and the JSON-object guard removes one outer pattern
level. Consuming dictionary removal avoids cloning the two JSON payloads.
This is a capability/usability demonstration, not a claim of fewer total lines:
the program now demonstrates two extra storage behaviors.

What does not become simpler is equally important. `dict.get` still requires
clone-safe values, so it cannot retrieve a packed Tool. Without Batch 2 entry
loans, dispatch must remove, call, and reinsert its tool, and therefore takes
a `mut` registry. Reinsert before returning either `Ok` or `Err`; do not use
`try tool(...)` before reinsertion. Removal/reinsertion can change insertion
order, and a trap terminates the operation without a rollback promise. This
program never iterates registry order. Shared concurrent registry access,
borrowed session callbacks, derived schemas, decorators, generators, and
more capable cleanup remain later work. Explicit JSON schema matching,
Result handling, metrics, retry, Queue streaming, and Session cleanup remain.

The phase 1 implementation checkpoint must publish the actual diff that adds
Tool, the factory, bound receiver, and remove/call/reinsert while still using
the **current Option library patterns**. The phase 2 diff then removes those
patterns and adds the narrowing/Lookup version above. Preserve the under-400-
line limit and the pinned stdout test on both backends at each checkpoint.
Report lines, allocation counts, executable bytes, and semantic differences
separately; this proposal provides no new measured performance claim.

## G. Prior art

**Python typing.** Adopt `type Name[T] = ...`, `T | None`, `is None` tests,
and `*` keyword-only boundaries for familiarity. Python's callable
specification treats names, parameter kinds, and default availability as
signature information; its basic `Callable[[...], R]` cannot express all of
that, so Aura embeds a declaration-shaped contract. Reject gradual `Any`,
implicit union subtyping/widening, and Python's runtime object ownership model
because Aura needs finite layouts and explicit ownership. The recommended
default-availability marker borrows signature notation, while fresh default
evaluation remains Aura's ADR-0015 rule. See the official
[callable specification](https://typing.python.org/en/latest/spec/callables.html),
[alias specification](https://typing.python.org/en/latest/spec/aliases.html), and
[union specification](https://typing.python.org/en/latest/spec/concepts.html#union-types).

**Rust closures and Option.** Adopt the distinction between shared, mutable,
and consuming calls and the requirement that transfer depends on captured
state. Rust's concrete closure types show why inline storage can be cheap;
an erased open callable requires additional dispatch/storage policy. Rust's
nominal Option preserves nested Some/None distinctions and documents eligible
niche optimizations. Aura deliberately chooses idempotent anonymous optionals
instead, so Lookup/Poll retain distinctions where flattening would destroy
them. Reject importing Rust's closure syntax or making heap trait objects
mandatory; keep Aura's established by-value capture default and explicit loan
lists. See the [Rust closure reference](https://doc.rust-lang.org/reference/types/closure.html)
and [Option documentation](https://doc.rust-lang.org/std/option/).

**Swift optionals and escaping closures.** Adopt Swift's attention to whether
a closure survives the creating call and whether an optional has been checked
before payload access. Its escaping-closure rules and capture discussion make
the environment's lifetime an API issue, not just a code-generation detail.
Aura rejects implicit reference-sharing of captured mutable state and uses
ownership/loan capabilities instead. Swift's `?` denotes its nominal Optional;
Aura's approved sole optional spelling is `T | None`, so it cannot preserve
Swift-style nested optional identity by retaining hidden outer tags. See the
[Swift closure guide](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/closures/),
[optional binding guide](https://docs.swift.org/swift-book/LanguageGuide/TheBasics.html),
and [type reference](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/types/).

**Mojo.** Adopt the systems-oriented lesson that capturing code and origin
tracking must compose: Mojo's official lifetime documentation explicitly
describes origin sets for captured values, and its changelog records evolving
closure/origin contracts. Aura should likewise avoid erasing capture
provenance merely to accept storage. Reject importing a moving experimental
closure spelling or treating external/unchecked origins as an escape hatch
for Aura loans. Keep the small owned subset now and design stored loans
jointly with collection places. This is a design comparison, not a claim that
a particular Mojo preview syntax is stable or implemented in Aura. See
[Mojo lifetimes and origins](https://docs.modular.com/mojo/manual/values/lifetimes)
and the [official changelog](https://docs.modular.com/mojo/changelog).

## H. Implementation and acceptance plan

### H1. Phase 1 — type foundations and owned callables

Begin only after ratification is folded into ADR-0052/0058 and the identified
dependent contract notes. Implement the E extraction and incremental backend
boundary alongside failing feature tests. Keep Option library removal outside
this phase so the intermediate usability checkpoint is concrete and reviewable.
Phase 1 includes conditional narrowing and the FFI/generic criteria rather
than postponing foundational correctness until the removal patch.

| Fixture family to add first | Parse/check expectation | Runtime oracle and parity |
| --- | --- | --- |
| `union_syntax_*`, `alias_*` | Parse-pass for precedence, nesting, contextual words, generic/public aliases; parse-fail AU1101; check-fail unknown names AU2001, cycles AU2012, invalid members AU2010 | Reordered/duplicate aliases share type and tags; singleton output equals its member; imported interfaces agree |
| `union_injection_*` | Check-pass at every written boundary; check-fail absent/ambiguous context AU2010/AU2011; no mixed-inference widening | Side-effect counter is `1` after one injection; integer/float tags remain distinct |
| `union_narrow_*` | Positive locals/params/fields/views, negation/and/or and return/loop edges; negative stale/overlap uses AU2014/AU3002, capability violations AU3003/AU3004 | Present/absent branches print pinned `present`/`missing`; mutation prints updated payload immediately; disjoint changes preserve access |
| `union_match_*` | Positive nested/guard/or-pattern/own/mut coverage; AU2013 for missing/duplicate/nonmember coverage; AU3001 for consumed roots | Pin one-time scrutinee/guard evaluation, committed ownership, every exit path, and tag-preserving mutable payload changes |
| `union_properties_*`, `union_generic_*` | Copy/clone/Transfer/equality positives and negatives; unresolved/colliding generic member proofs AU2010/AU2013; no ordering AU2003 | Tagged equality differs for same numeric magnitude; nested optional flattening prints one None; tagged-presence controls remain distinct |
| `union_member_equality_*` | Both operand orders and None pass for eligible unions; ambiguous literal AU2011; nonmembers, different union types, or an incomparable member AU2003; equality alone does not narrow | Branches print pinned `equal`/`different` labels; each operand runs once; internal hash assertions equate union/member hashes, including None and signed zero, while different tags remain unequal |
| `callable_store_*`, `callable_merge_*` | Shared/Mutable/Consuming through params/results/fields/list/dict/imports; AU2015 contract/erasure failures; AU3010 loan escape | Factory-owned state survives return; Shared repeated reads print `2`, `2`; Mutable owned counter prints `1`, `2`; Consuming yields one value |
| `callable_storage_*` | Inline/overflow and zero-capture shape checking; non-cloneable collection get AU3007, moved callable AU3001 | Allocation counters: zero new environment allocations for fitting capture, one for overflow; failed packing AU4005; prefix cleanup order pinned |
| `bound_method_*`, `callable_view_result_*` | Owned/Copy/shared/mut/own receiver cases; reject non-Copy borrowed binding and hidden self-origin AU3010 | Copy snapshot mutations do not change original; moved receiver cleanup once; explicit argument-origin view writes through |
| `callable_binding_*`, `callable_forwarding_*` | Names, named/default/keyword-only declarations/contracts, generic and trait conformance; AU2004 binding errors, AU2015 metadata mismatch; no builtin wrap member | Trace supplied arguments, outer defaults, wrapper entry, then each ordinary inner call; supplied inner slots suppress defaults, omitted slots evaluate the selected target's defaults freshly per inner call |
| `stored_task_target_*` | All start methods; concrete structural Transfer passes; forbidden captures/results AU3008; ordinary mut arguments AU3004 | Stored target moved once; parent defaults finish before child scheduling; task ancestry/diagnostic identity preserved |
| `ffi_nullable_handle_*` | Package-authorized exact handle/None results pass; scalar/multiple-member/parameter unions AU2010; invalid opt-in uses existing code | Linked C test shim returns null/non-null; optional null is None, non-optional null AU4005; mutable bytes write back before result validation |

Test both controlled failure and normal cleanup without making a timing
benchmark the correctness oracle. Use existing test allocation/cleanup hooks
where available; any new hook must enable a concrete observable contract test,
not change production behavior. Tests of malformed serialized metadata or
security-sensitive containment follow AGENTS.md's standing defensive-review
delegation requirement during implementation; this task performs no such
investigation or implementation.

LSP work covers alias definition/navigation, complete callable hover, active
member hover at each flow edge, keyword completion/argument help, type-pattern
completion, precise diagnostics, imported defaults, rename/reference behavior,
and schema invalidation. Preserve compiler-owned semantics and 100% LSP
coverage. The extension only adds grammar/token/snippet support for contextual
forms and verifies its package/build; it must not infer a second type system.

Manual chapters to update with implementation: Types, Functions, Closures,
Generics and Traits, Enums and Match, Ownership and Borrowing, Static Semantics,
Grammar, Lexical Structure, Names and Scopes, Expressions, Concurrency, FFI,
Diagnostics, Current Limits, Conformance, and API Index. Examples/tutorials,
reference-integrity hashes, generated LLM documents, compiler/CLI READMEs, and
the work board follow the actual implemented surface in the same change family.
Do not teach Display, collection-entry loans, or decorator syntax as delivered.

Phase 1 acceptance requires the complete feature matrix on MIR and forced
direct execution, semantic/MIR interface round trips and cache rejection of
old schema versions, exact source diagnostics, unchanged coverage floors,
100% LSP coverage, extension packaging, standalone release-archive smoke,
Manual/tutorial gates, and an actual reference-agent diff with pinned output.
Run one complete implementation verification and one complete green hosted CI
run; repeated green hosted runs are not required. Publish fixture and failure
counts without hiding unsupported feature cases behind automatic backend fallback.

### H2. Phase 2 — remove Option across the maintained surface

Add the failing replacement contracts before changing the 39 API signatures
or removing syntax. Follow D4's order. Carry all existing behavior tests
forward, changing expected source/output only where the ratified contract
actually changes, and preserve evidence for the before/after checkpoint.

| Fixture family to add first | Check/diagnostic coverage | Runtime oracle and parity |
| --- | --- | --- |
| `optional_api_*` | Each of D3's 39 signatures in direct and imported use; Option argument defaults/owned modes replaced | Present, absent and error outputs per API; empty text/bytes/datagrams remain present |
| `lookup_optional_payload_*` | `Lookup[None]`, `Lookup[int64 \| None]`, non-cloneable get rejection and remove acceptance | `Found(None)` and `Missing` print distinct qualified cases; removal transfers exactly once |
| `poll_optional_payload_*` | Queue/Task payload None and nested optional; generic specialization and observation restrictions | `Ready(None)` versus `Unavailable`; timeout/error/cancellation keep their collapsed policy and consume non-repeatable task observation rights |
| `optional_flow_library_*` | Read a returned optional into locals/fields/views; branch/loop mutation invalidation | EOF loops, optional pipe cleanup and JSON decoding print pinned outputs with correct resource lifetime |
| `unknown_type_and_member_*` | AU2001 for unknown Option type with no declaration, AU1101 for unexpected `?`, ordinary member errors for absent constructors/poll predecessors | No compatibility-mode or migration-specific diagnostics; an ordinary user enum named Option has ordinary user semantics |
| `optional_interfaces_*` | Generic exported signatures, aliases, callable defaults/results, rebuilt schemas | Both backends agree on optional tags and API carrier types through imported packages; stale artifacts are rejected normally |
| `reference_tool_runner_*` | Final source contains no language Option spelling and remains under 400 lines | Full stdout matches version 0 except version header; both owned closure and bound method dispatch run |

Phase 2 touches the phase 1 Manual chapters plus Collections, Numeric Arrays,
JSON, I/O, Control Plane, Network, Process, Classes, and Tuples; re-inventory
all Manual pages rather than treating this list as a search exemption.
Update every affected example/tutorial, analysis signature string, completion,
extension snippet, fixture sidecar, checked-interface test, and generated
document. Remove only the *language* Optional representation; preserve host
Rust Option and existing nominal Result/QueueReceive/TaskResult semantics.

Phase 2 acceptance additionally requires all D3 rows accounted for in tests,
absence-versus-present-None evidence for the five generic APIs, ordinary
diagnostics using ordinary unknown-name/parser rules, an inventory showing no
maintained builtin Option/`T?` surface, and the second actual reference-agent
diff. Run the same full parity, coverage, CLI, LSP, extension, reference,
tutorial, packaging, identity, and one-green-hosted-CI criteria. Refresh
performance/size/allocation baselines to identify regressions separately from
correctness; Batch 1 does not ratify a Rust-parity speed target.

### H3. Relative size estimates

These are planning ranges in fixture **files**, not elapsed-time estimates,
test counts already achieved, or caps on completion. Unit/interface and LSP
test cases are additional. Several rows may share a parameterized fixture,
but every observable combination above still needs an oracle.

| Work | Parse-pass | Check-pass | Check-fail | Run-pass | Run-fail | Touched modules/surfaces |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Phase 1 | 20–28 | 42–60 | 70–95 | 50–70 | 10–16 | About 20–30 Rust modules including the extracted sema modules, plus 5–8 LSP/extension files and roughly 17 Manual chapters |
| Phase 2 new replacement fixtures | 0–3 | 12–20 | 12–18 | 30–44 | 6–10 | About 12–18 Rust modules, overlapping phase 1, plus the builtin signature/tooling surfaces and roughly 25–30 Manual pages after final inventory |

Phase 1 is approximately 192–269 new fixture files; phase 2 is 60–95 plus
conversion of the existing Option fixtures (54 `.au` compiler fixtures under
the measured scope, with additional CLI/package/tutorial cases to inventory).
Backend parity covers every new run-pass/run-fail fixture, not a sampled
subset. The phase 1 representation/callable/flow work is substantially larger
than phase 2's surface conversion, but phase 2 spans more maintained prose
and API signatures. If the inventory grows, finish the required surface rather
than treating an estimate as a stopping condition.

### H4. Proposed diagnostics — Q24

**Recommend a focused AU2010–AU2015 family** for new type-foundation failures,
while reusing established parser, call-binding, ownership, task, and runtime
codes. These six codes are unused in the checked baseline; reserve them only
in implementation. Message templates below describe
semantics; substitute canonical types, paths, and names and attach origin/use
spans. The alternative uses existing AU2002/AU2999 for all new type errors,
reducing registry growth but making diagnostics less distinguishable in tools.

| Code | Proposed message template / use |
| --- | --- |
| AU1101 | `expected a type after '\|'`; `expected a named parameter after '*'`; ordinary malformed contextual syntax or unexpected `?` |
| AU2001 | ``unknown type `Option` `` when no such type is declared; ordinary unresolved alias/type/member names |
| AU2002 | Existing exact argument/result mismatch, generic arity/bounds and unsupported ordinary FFI type |
| AU2003 | `cannot compare '{U}' and '{T}': expected the same union type or an eligible member`; incomparable union member, unsupported union ordering/hash or callable equality; ambiguous literals use AU2011 |
| AU2004 | `parameter '{name}' is keyword-only`; `missing required argument '{name}'`; duplicate/unknown argument, default on mut/origin/trait slot |
| AU2010 | `union value requires an explicit expected type`; `'{T}' is not a member of '{U}'`; invalid/incomplete union member; forbidden FFI union shape; ambiguous inverse generic normalization |
| AU2011 | `literal matches multiple members of '{U}': {members}; annotate one member before injection` |
| AU2012 | `cyclic type alias: {alias path}`; report the closing dependency and each involved declaration |
| AU2013 | `match does not cover {members}`; duplicate/unreachable/nonmember type arm; generic type arm whose direct membership/disjointness is unproved |
| AU2014 | `narrowing of '{place}' no longer applies after {operation}; test the current value again`; include test and invalidation spans |
| AU2015 | `callable contract mismatch: {name/default/keyword/kind/obligation difference}`; implicit erased storage or an invalid explicit restriction; missing common branch contract |
| AU3001 | Existing use-after-move, including bound receiver creation, consumed optional root, and Consuming callable invocation |
| AU3002 | Existing overlap/locked-owner/shared-move failure; source and live payload loan spans |
| AU3003 / AU3004 | Existing mutation-through-shared / invalid mutable place or parameter capability, including Mutable callable access |
| AU3007 / AU3009 | Existing forbidden clone/collection observation, including callable environments or single-consumer task rights |
| AU3008 | Existing structural task/Queue rejection, including the precise TaskCallable capture/result path |
| AU3010 | Existing loan closure escape, invalid returned origin, or attempted bound-callable self-origin |
| AU4005 | `cannot allocate callable environment`; existing non-null FFI result and marshalling failures |

Use AU3002 for an actual live-loan conflict; use AU2014 only when a prior
refinement was valid and has since been invalidated without a still-live
loan. Plain access that was never narrowed gets the ordinary type/member
error. No dedicated old-to-new Option diagnostic or automatic fix is added.

## Reconciliation and confidence

| Records pulling in different directions | Ratified resolution and remaining reconciliation |
| --- | --- |
| Approved Decision 2 versus ADR-0052's one-member rejection and unspecialized-generic baseline | Idempotent flattening requires designed singleton/generic behavior; Q1/Q7 collapse singleton substitutions and distinguish a type parameter from a bare generic constructor |
| ADR-0052's unconstrained-None rejection versus implemented unit None | Keep existing standalone unit None; it never infers an optional member set |
| ADR-0052's tag-inclusive hashing versus ratified Q6 B union/member equality | A6 delegates union hashing to the active member so equal union/member values hash equally; different-tag collisions are permitted. Amend the ADR hashing paragraph before implementation |
| Approved Decision 2 versus Option's nested present-None distinctions and ADR-0033 observations | Q23 A's Lookup/Poll preserve information and the existing conservative task observation right; Batch 2 revisits app-facing optional dict.get with element loans and an explicit present-None policy |
| ADR-0052's blanket C-union exclusion versus the roadmap nullable-result criterion | Q10 makes exactly handle-plus-None a result marshalling exception; C still receives one pointer. FFI v0 has no Option ABI to preserve |
| ADR-0037's capture-free stored def, read-only owned captures, and branch-merge rejection versus Approved Decision 6 / ADR-0058 | Q13/Q14/Q17 introduce explicit erased ownership and Mutable environments; concrete and thin function categories keep their existing ownership meaning |
| Approved Decision 6's full eventual storage promise versus ADR-0038/0061's lifetime restrictions | C10 retains the explicit owned-only delivery split; Q22 exposes only ordinary argument-origin returned views, not stored loans or captured-self origins |
| ADR-0033 derives Transfer from complete state, while an open Callable hides that state | Q21 checks the environment before erasure into TaskCallable; ordinary erased Callable cannot recover that proof or assert it through a user trait |
| ADR-0051/implemented function types erase names/defaults, versus ADR-0058's preserved keyword/default contract | Q17/Q19 make binding metadata part of type identity and allow only explicit restrictions; no assignment turns keyword-only into positional |
| ADR-0015's target-specific fresh defaults versus heterogeneous storage and wrapper forwarding | Q19 retains per-target default references outside structural identity; Q20 B adds no helper, so wrappers declare their own contracts and defaults follow each ordinary call boundary |
| ADR-0052's all-member trait rule versus ADR-0055's structural rendering fallback | Q8 separates rendering eligibility from nominal trait satisfaction; no early Display implementation or purity claim |
| General exact-once cleanup promises versus ADR-0038's forced-frame-reset boundary | A9/C4 preserve host containment without promising arbitrary source cleanup on destroyed frames; the existing Batch 3 cancellation conflict is not resolved here |

The ten Approved Decisions are constraints throughout this table, not subjects
being reopened. Later ADR edits must distinguish earlier implementation
limits from changes to those accepted directions.

Proposal commit `fe9c6c0` identified three least certain recommendations:
**Q15's three-word buffer**, because measurements may favor another capacity;
**Q20 A's wrap operation**, because exact default forwarding added a
signature-aware primitive before general decorators; and **Q23 A's Lookup/Poll
naming split**, because uniform presence semantics add ceremony for concrete
non-optional payloads. Ratification accepts Q15 A, selects Q20 B and therefore
removes that operation, and accepts Q23 A with the explicit Batch 2 app-facing
dictionary lookup review. The buffer remains an accepted design choice whose
allocation behavior must be measured during implementation.

The questionnaire covers each ratified syntax/observable contract bundle:
A1–A10 map to Q1–Q10, B1–B2 to Q11–Q12, C1–C9 to Q13–Q22, D's replacement
API spellings to Q23, and H4's diagnostic policy to Q24. Section E contains
engineering extraction choices only. Phase ordering, clean-slate removal,
fixed equality exclusions, maintained tests/backends, and deferred loan
storage are required constraints rather than optional answers.

## Ratification questionnaire

Ratification recorded on 2026-09-08: **all recommended, except Q6: B and
Q20: B**; **Q23: A** includes the required Batch 2 app-facing dictionary lookup
review. The original options and single recommendation per question are
retained from proposal commit `fe9c6c0`. Each **Ratified** line records the
user's selected answer; the sections above reconcile the dependent contracts,
examples, diagnostics, and implementation tests with those answers.

### Union rules — Q1–Q10

**Q1 — A1: What happens after normalization leaves one member?**

- A. Collapse to that member, including unit None; generic substitution stays compositional.
- B. Reject a redundant one-member union; users must avoid duplicate-producing specializations.
- **Recommended: A.**
- **Ratified: A.**

**Q2 — A2: How should ambiguous injection and sub-expression annotation work?**

- A. Require exactly one member; use existing numeric casts or a hoisted typed local/helper when context does not reach the expression.
- B. Keep unique-member injection but extend `as` to general static type ascription; shorter expressions gain another meaning of `as`.
- **Recommended: A.**
- **Ratified: A.**

**Q3 — A3: Which places narrow, and when do facts expire?**

- A. Use the full existing root/fixed-field/tuple/view place model with generation, overlap, loan, and mutable-call invalidation; capabilities remain unchanged.
- B. Narrow only locals and parameters; fields and views require explicit matching or owned snapshots.
- **Recommended: A.**
- **Ratified: A.**

**Q4 — A4: How much conditional flow narrowing should Batch 1 support?**

- A. Contextual `is None`/`is not None`, negation, short-circuit `and`/`or`, and reachable return/break/continue joins; no identity or truthiness narrowing.
- B. Support only direct optional tests; Boolean combinations require nested control flow.
- **Recommended: A.**
- **Ratified: A.**

**Q5 — A5: How should type patterns and mutable narrowing behave?**

- A. Use `case Type as name` with existing shared/own/mut match modes, exhaustive direct-member coverage, and tag changes only through an unlocked whole union place.
- B. Add shared/owned type patterns first and defer mutable type-pattern bindings; mutation uses whole-union replacement outside matching.
- **Recommended: A.**
- **Ratified: A.**

**Q6 — A6: May a union compare directly with a member?**

- A. Require the same normalized type for equality, derive tag-sensitive equality/hash across all members, and use `is None` for absence tests.
- B. Contextually inject member operands for symmetric equality; comparisons inherit injection ambiguity and require a carefully scoped hashing law.
- **Recommended: A.**
- **Ratified: B.**

**Q7 — A7: How should generic union members specialize?**

- A. Admit declared type parameters, renormalize after substitution, and check generic narrowing/patterns universally; optional substitutions flatten.
- B. Admit `V | None` but reject optional substitutions for V; generic API composition needs additional constraints or explicit tagged specialization.
- **Recommended: A.**
- **Ratified: A.**

**Q8 — A8: Does structural rendering imply nominal Display satisfaction?**

- A. Keep all-member nominal trait obligations distinct from active-payload structural rendering; Batch 4 can attach the shared Display dispatcher.
- B. Let structural rendering satisfy Display bounds automatically; generic display is easier but changes nominal trait coherence.
- **Recommended: A.**
- **Ratified: A.**

**Q9 — A9: Should Batch 1 use union niches?**

- A. Use one shared explicit-tag layout now; retain the niche clause only for a later centrally specified, ABI-consistent optimization.
- B. Also implement eligible nullable-handle niches now; some values shrink but the first backend/layout matrix grows.
- **Recommended: A.**
- **Ratified: A.**

**Q10 — A10: What is the nullable FFI-result boundary?**

- A. Permit exactly `Handle | None` as an extern result marshalled to one C pointer; keep every other union FFI shape excluded.
- B. Keep all union extern annotations excluded; nullable native APIs need reviewed C shims using existing scalar/status and non-null-handle contracts.
- **Recommended: A.**
- **Ratified: A.**

### Alias rules — Q11–Q12

**Q11 — B1: Which alias declaration surface should land?**

- A. Contextual module-level `type Name[T] = T`, existing `public`/imports/bounds, transparent expansion, and rejection of every alias-expansion cycle.
- B. Start with the same module/visibility/cycle rules but nongeneric aliases only; common generic signatures remain repetitive.
- **Recommended: A.**
- **Ratified: A.**

**Q12 — B2: How should aliases appear in tools and hashes?**

- A. Retain source/import alias names plus canonical expansions for diagnostics/hover, while semantic keys use expansion and interface hashes include exported alias records.
- B. Erase alias presentation after resolution; canonical expanded types simplify tooling but lose users' chosen names.
- **Recommended: A.**
- **Ratified: A.**

### Callable rules — Q13–Q22

**Q13 — C1: Extend `def` storage or introduce an owned callable type?**

- A. Keep thin Copy `def` values and add explicit `Callable[def(...)]`, `Callable[mut def(...)]`, and `Callable[own def(...)]` packing.
- B. Extend stored `def` itself to environments and call kinds; signatures shorten but general stored function values become affine and larger.
- **Recommended: A.**
- **Ratified: A.**

**Q14 — C2: How should owned mutation and call-kind restrictions work?**

- A. Infer Shared/Mutable/Consuming from owned-capture use and permit explicit Shared-to-Mutable/Consuming or Mutable-to-Consuming restriction adapters.
- B. Infer the same kinds but require exact-kind assignment/packing; every restriction needs a source wrapper.
- **Recommended: A.**
- **Ratified: A.**

**Q15 — C3: What owned callable storage/allocation policy should land?**

- A. Use a three-word inline environment plus one table pointer, with one checked overflow allocation and AU4005 on controlled allocation failure.
- B. Require explicit inline capacity and reject oversized packing; allocation is avoided but capacity becomes part of public contracts.
- C. Require statically enumerated environment families with tag dispatch; allocation is avoided but factories cannot hide an open set of captures.
- **Recommended: A.**
- **Ratified: A.**

**Q16 — C4: How should erased destruction be represented?**

- A. Use one per-value operations-table pointer with an indirect drop entry; reverse capture destruction shares metadata across values.
- B. Store invocation and destructor pointers directly per value; one lookup is saved at the cost of at least one additional word.
- **Recommended: A.**
- **Ratified: A.**

**Q17 — C5: How should heterogeneous storage and contract conversion work?**

- A. Require an explicit common contract for branch/storage merges, invariant types, and explicit safe name/default/keyword restrictions; never make keyword-only positional.
- B. Allow safe restrictions implicitly during assignment and joins; code is shorter but exposed metadata can disappear through inference.
- **Recommended: A.**
- **Ratified: A.**

**Q18 — C6: How should owned or Copy methods bind?**

- A. Use `receiver.method`, moving an owned receiver or snapshotting a Copy receiver; method receiver capability determines call kind and mutates only captured state.
- B. Require an explicit `receiver.bind(method=Type.method)` operation; capture is more visible but needs a new selector operation.
- **Recommended: A.**
- **Ratified: A.**

**Q19 — C7: What belongs in callable default/name/keyword identity?**

- A. Include names, modes, `*`, and `= ...` availability in structural identity; retain target-specific default code separately and evaluate it freshly after supplied arguments.
- B. Also include default-expression identity in the type; unrelated defaulted targets cannot share one structural storage contract.
- **Recommended: A.**
- **Ratified: A.**

**Q20 — C7: How should transparent wrappers preserve selected defaults?**

- A. Add the concrete-signature `base.wrap(body)` adapter: bind defaults once before the body and pass an all-required target contract for forwarding.
- B. Add no wrapper helper; handwritten wrappers supply inner arguments and explicitly define their own default policy.
- **Recommended: A.**
- **Ratified: B.**

**Q21 — C8: How should erased callables retain Transfer evidence?**

- A. Add compiler-checked `TaskCallable[contract]`; ordinary erased Callable is non-Transfer, while admitted task callables move into child-owned invocation storage.
- B. Add a compiler-owned Transfer constraint slot to Callable; one constructor remains but its marker must be distinguished from an ordinary user trait.
- **Recommended: A.**
- **Ratified: A.**

**Q22 — C9: May stored callables return views in Batch 1?**

- A. Permit only the existing single explicit argument-origin view contract; reject captured-self origins and every stored loan environment.
- B. Defer all stored view-returning callables; those functions remain direct-call-only for now.
- **Recommended: A.**
- **Ratified: A.**

### Library and diagnostic rules — Q23–Q24

**Q23 — D2–D4: Which tagged library replacements preserve present None?**

- A. Use `Lookup.Found/Missing` for list/dict lookup/removal and `Poll.Ready/Unavailable` for renamed Queue/Task `poll`; use `T | None` for the remaining audited optional positions.
- B. Use separate nominal result types for each lookup/poll operation; distinctions remain exact but more type names and handlers are required.
- **Recommended: A.**
- **Ratified: A.**
- **Required follow-up:** Batch 2 element-loan design revisits an app-facing `V | None` form of `dict.get`.

**Q24 — H4: Which diagnostic policy should accompany the design?**

- A. Add AU2010–AU2015 for union, alias, narrowing, and callable-contract errors; reuse existing ownership/binding/runtime codes and ordinary AU2001 for unknown Option.
- B. Use existing AU2002/AU2999 for every new type-foundation failure; the registry stays smaller but tools distinguish fewer causes by code.
- **Recommended: A.**
- **Ratified: A.**
