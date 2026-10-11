# Batch 3 Phase 3a: Initialization

## Goal

Implement custom initialization as ratified in
`architecture_docs/17-batch-2-3-design-checkpoint.md` section E (Q12-Q16,
option A) and ADR-0059: `__init__(self, ...)`, field defaults evaluated
before the body, definite initialization, initializer visibility, named
fallible factories, and exact-once cleanup of a partially built object.

## Work Completed

| Area | Change |
| - | - |
| Diagnostics | `AU2016` initializer declaration and `AU3012` definite initialization join the registry. |
| Checker | `validate_initializer_declaration` rejects a non-bare receiver, own type parameters, a non-`None` result, and a trait member named `__init__` (`AU2016`). `__init__` used as a method or value is `AU2005`. Moving `self` is `AU3004`. A cross-module call of a private initializer is `AU2001`. The class call is checked against the initializer contract without its receiver. |
| Definite initialization | New `sema/init.rs` runs a per-field `Uninit`/`Init`/`MaybeInit` analysis with branch joins, loop fixed points, `while true` exits through `break`, early returns, and lambda captures. |
| MIR | `C.__init__` lowers to a receiver-less function that returns `C`. Its entry allocates `self` with only the defaulted fields. Returns move `self` out. The class call lowers to a call of `C.__init__`. Statement matches inside an initializer end in an unreachable block. |
| Validator | The shared loan-flow validator checks the slot: a partial `Construct` appears only as the initializer's single `self` allocation, no path reads or lends an absent field, `self` as a whole needs every field on every path, and no edge re-enters the allocating block. A constant branch has one successor. Callable-identity checks skip absent fields. |
| Backends | The interpreter's instance map lacks absent fields. The direct backend fills absent plain-class words with zero and builds opaque instances field by field. |
| Tooling | Signature help on a class call shows the initializer contract. Class hover lists `def __init__(self, ...)`. Class completion detail shows `C(params)`. Member completion omits `__init__`. Semantic interface schema 18 in the compiler and language server. |
| Docs | Classes gains an Initializers section and updated typing, runtime, diagnostics, limits, and status. Diagnostics, Static Semantics, Execution Model, Grammar, Types, Current Limits, From Python, and tutorial chapters 5 and 14 are updated. |
| Examples | `examples/classes/initializers.au` and `examples/classes/factories.au`. |

### Deviation From G1

The checkpoint's G1 asked for typed `DropInitialized` exit actions. Phase 3a
represents the object under construction as a value whose undefaulted
fields are absent, and drops it as an ordinary owned local on a trap or
cancellation. Aura has no user destructors, so this releases exactly the
fields present, once, without a second drop path. The validator proves the
slot discipline instead of action pairing. ADR-0059's implementation record
gives the reasoning and what a later feature with observable per-field
cleanup would need.

### Narrowing From E4

E4 allows an `own` use of an initialized field that returns it to
`Uninit`. Phase 3a rejects every move out of `self` in an initializer with
`AU3002` and initializer-specific help. This is documented in Classes and
Current Limits.

### Pre-Existing Defect Found

Calling a callable stored in a class field through a shared `self` in an
ordinary method fails MIR validation with "has no authoritative callable
contract". This reproduces on `main` without initializers. The
`init_helper_projection` fixture avoids the shape. It needs its own fix.

## Verification

- Fixtures: 16 run-pass (`init_*`, `factory_result_init`), 24 check-fail,
  2 check-path-fail, and 1 run-fail. Every run-pass fixture matches on the
  forced MIR and direct backends.
- `crates/aura-compiler/tests/batch3_initializer_validator.rs`: 12
  forged-MIR cases refused identically at `run_mir` and
  `emit_host_native_object`.
- `failed_factories_release_builtin_file_handles_on_both_backends` runs
  300 failing factory calls under a 64-descriptor limit on both backends.
- Analysis unit test for signature help, hover, and completion.
- Full gate results are recorded in the task board entry.

## Follow-Up

- Phase 3b, context managers, lands next on the same branch.
- Fix the callable-field call through shared `self`.
- Moving a field out of `self` and reinitializing it inside `__init__`.
