# Batch 6 Design Checkpoint

## Goal

Plan roadmap Batch 6, typed agent APIs and serialization, before any
implementation. John chose to start this checkpoint after the 0.5.0 preview
release.

## Work Completed

- Surveyed what the compiler offers a code generator: traits with default
  and associated methods, generated MIR helpers, docstrings on fields and
  classes, eagerly initialized module constants, the `@property` marker, and
  owned callables.
- Found two gaps. A generic function cannot call `T.decode(...)` through a
  bound, and ADR-0053 rejects generic decorators.
- Wrote `architecture_docs/21-batch-6-design-checkpoint.md`. It proposes:
  - an empty `impl json.Codec for T` as the opt-in, with an explicit body for
    classes that have `__init__`
  - strict decoding with `json.DecodeError` paths such as `$.items[2].name`
  - a JSON Schema 2020-12 subset that matches the decoder
  - decorator order, recursion through the module slot, generic decorators
    inferred per use, explicit tool lists, and the `@retry` ownership rule
  - an eight-step delivery plan
- Linked the checkpoint from roadmap row 6.

## Verification

Documentation only. No compiler behavior changed.

## Follow-Up

- John ratifies or changes the seven owner questions.
- Step 1 adds associated-method calls through a bounded type parameter.
