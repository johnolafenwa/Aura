# ADR-0059: Custom initialization and fallible factories

- Status: Accepted; detailed design ratified in the Batch 2-3 checkpoint (Q12-Q16, Q23)
- Date: 2026-09-06; implementation record 2026-10-11
- Implementation: Implemented in Batch 3 phase 3a, with the slot deviation recorded below
- Roadmap: Batch 3
- Related: ADR-0005, ADR-0015, ADR-0022, ADR-0058, and ADR-0060

## Authority and current boundary

The user approved `__init__(self, ...)` in the
[roadmap](../14-priority-roadmap.md). Aura currently constructs classes from
their fields and per-instance defaults; `__init__` has no constructor meaning
in the implemented language.

## Accepted decisions

- A class defining `__init__(self, ...)` uses that initializer's parameters for
  class calls. `self` denotes the object being initialized and is not supplied
  by the caller.
- A class without an initializer retains automatic field-based construction.
- Every required field must be initialized on every successful construction
  path. The compiler rejects reads of uninitialized fields and escape of a
  partially initialized object, including through callbacks, tasks, or storage.
- Initializer execution may validate inputs and compute fields. Failure must
  clean up initialized fields and owned temporaries exactly once; it cannot
  expose a partially initialized instance.
- Fallible construction initially uses named factories returning
  `Result[Class, Error]`. An initializer returning a fallible construction
  result is not part of the initial accepted surface.
- Field/argument types and ownership remain statically checked. Custom
  initialization does not introduce dynamic attributes or inheritance.

## Remaining detailed design

### Open conflicts

- **Batch 3, ADR-0038 / ADR-0060 / ADR-0062:** use one shared partial-construction
  cleanup mechanism extending ADR-0038's ordered exit-action stack. Initializer
  failure, multi-resource entry failure, and failed decodes must reuse it for
  exact-once cleanup; do not build separate mechanisms for each feature.
- **Batch 6, ADR-0062:** computed `__init__` fields have no general inverse.
  Typed decoding applies to classes with automatic field-based construction
  or an explicit decode factory. Factory spelling is to be designed in Batch 6.

### Detailed contract

Define the initializer result/signature restrictions, generic and visibility
rules, interaction with declaration defaults, evaluation order, and delegation
or alternate-constructor policy. Definite initialization must cover branches,
loops, early exits, nested fields, and cleanup after partial construction.

`self` uses initializer-specific authority to establish fields. Specify that
authority and its transition to ordinary initialized ownership explicitly;
the spelling does not change the shared meaning of bare `self` on ordinary
methods. Decide whether helper calls can receive initialized projections
before completion and how their effects are checked. A factory must not bypass
private-field rules or construct an invalid object on its successful path.

## Completion evidence required

Fixtures must cover default constructors, custom argument binding, computed
fields, missing/duplicate initialization, read-before-initialization, attempted
partial-object escape, branch coverage, and exact-once partial cleanup.
Run-pass factories must preserve typed errors and return fully initialized
objects. Pin generic/imported constructors, ownership, evaluation order, both
backends, interface metadata, and editor constructor signatures. Update class
examples and the Manual only when implementation lands.

## Implementation record (2026-10-11)

The detailed contract is
[checkpoint section E](../17-batch-2-3-design-checkpoint.md#e-custom-initialization),
ratified with option A for Q12-Q16. Phase 3a implements it as follows.

- **Declaration and call.** `__init__` stays in the class's method table.
  `AU2016` rejects a receiver other than bare `self`, own type parameters,
  a non-`None` result, `return value`, and a trait member named `__init__`.
  `AU2005` rejects `x.__init__` and `C.__init__` as a value. The class call
  is checked as a call of the initializer contract without its receiver.
  A private initializer rejects a cross-module class call with `AU2001`.
- **Definite initialization.** `sema/init.rs` runs a per-field
  `Uninit`/`Init`/`MaybeInit` analysis over the initializer body before
  ordinary checking. Joins and loop fixed points follow E4. `while true`
  exits only through `break`. Diagnostics are `AU3012`. A lambda that
  mentions `self` counts as a whole-object use.
- **Object slot (deviation from G1).** `C.__init__` lowers to a
  receiver-less MIR function that returns `C`. Its entry assigns
  `self = Construct { fields: defaulted fields only }`, so undefaulted fields
  are absent from the value rather than tracked by `DropInitialized` exit
  actions. The interpreter's instance map lacks the absent fields. The direct
  backend builds an opaque instance with `instance_empty` and `set_field`,
  and fills absent plain-class words with zero. A trap or cancellation drops
  the partial object as an ordinary owned local, which releases only the
  fields present, exactly once. Completion returns `MovePlace("self")`.
  The shared MIR validator proves the slot discipline instead of exit-action
  pairing: a partial `Construct` appears only as the initializer's single
  `self` allocation, no path reads or lends an absent field, `self` as a
  whole is used only after every field is present on every incoming path,
  and no edge re-enters the allocating block. Forged-MIR refusals are pinned
  in `crates/aura-compiler/tests/batch3_initializer_validator.rs` at both
  `run_mir` and `emit_host_native_object`.

  The deviation keeps cleanup exact-once without a second drop path, because
  Aura has no user destructors and every field drop is an ordinary value
  drop. It does not give Batch 6 decoders a reusable `DropInitialized`
  action. A decoder that constructs into a partial slot can reuse the same
  absent-field representation and validator rule. If a later feature needs
  ordered per-field cleanup with observable effects, it must add the G1
  action then.
- **Moves out of `self`.** E4 permits an `own` use of an initialized field
  that returns it to `Uninit`. Phase 3a rejects every move out of `self`
  instead (`AU3002`, with initializer-specific help). Moving a field out and
  reinitializing it remains a possible extension.
- **Factories.** Named factories are ordinary associated methods. A CLI test
  runs three hundred failing factory calls under a 64-descriptor limit on
  both backends, which verifies that builtin `fs.File` locals dropped on a
  failing `try` release their host handles.
- **Tooling.** Signature help on a class call shows the initializer
  contract. Class hover lists `def __init__(self, ...)`. Class completion
  detail shows `C(params)`. Member completion omits `__init__`. The semantic
  interface schema is version 18.
