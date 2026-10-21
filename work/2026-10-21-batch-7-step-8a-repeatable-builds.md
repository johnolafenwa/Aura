# Batch 7 Step 8a: Repeatable Native Builds

## Goal

Make two builds of the same program emit the same native object. This is
step 8a of the [Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md),
the first step of moving the direct emitter's semantics into MIR. Later
sub-steps compare emitted code before and after a change, which needs
repeatable output.

## Finding

A new test lowered and built every run-pass fixture twice in one process.
538 of 626 fixtures emitted different objects. MIR was already identical.
Each `HashMap` and `HashSet` gets its own random seed, and codegen iterated
three kinds of map in ways that reached the object:

- `release_all_opaque_roots` released a function's roots at return in
  `variable_types` order.
- `OwnedTemporaries` released a statement's temporaries in `HashSet<Value>`
  order.
- `define_function` declared callees in `functions` and `function_thunks`
  order, and `define_callable_shapes` defined shapes in `callable_shapes`
  order.

## Work Completed

- Roots are released in name order at return.
- `OwnedTemporaries` keeps `BTreeSet` and `BTreeMap`, so temporaries are
  released in value order.
- Callees are declared, and callable shapes defined, in sorted order.
  `CallableShapeKey` derives `Ord`.
- `direct_objects_are_identical_across_repeated_builds` lowers and builds
  every third run-pass fixture twice and compares the MIR and the object
  bytes. A third keeps the debug test short next to the timing-sensitive
  task tests.

## Verification

- The repeat-build test passed for all 626 fixtures before it was cut to
  every third one.
- Clippy, the compiler, CLI, and runtime suites, and the backend parity
  matrix run before the PR.

## Follow-Up

- Step 8b adds one runtime ABI table for every emitter.
