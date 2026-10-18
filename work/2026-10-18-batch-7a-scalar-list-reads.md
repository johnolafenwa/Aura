# Batch 7a Step 6: Unboxed Scalar List Reads

## Goal

Read `list[int64]`, `list[float64]`, and `list[bool]` elements in native
code without allocating a box per element. This is the first part of
section E in the
[Batch 7a design checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md).

## Work Completed

- `aura_direct_vec_index_scalar(list, index, kind)` in the native runtime
  borrows the list and returns the element as a machine word. Kind 0 is
  `int64`, 1 is `float64` as its bit pattern, and 2 is `bool`.
- The native code generator uses it for the internal `__index` read when the
  element type is one of those scalars. The bounds guard stays in generated
  code, so out-of-bounds diagnostics are unchanged.
- Fixture `run-pass/scalar_list_reads.au` covers `for` loops, positive and
  negative indexing, and a `while` index loop on all three element types.

## Verification

- `scalar_list_reads_return_unboxed_bits` and four new cases in
  `direct_runtime_helper_errors_surface_expected_diagnostics`.
- `cargo test -p aura --test backend_parity --test native_codegen_acceptance
  --test representation_stats`.
- Callgrind on `int_list_sum`: 2.42 G to 0.55 G instructions. Wall time
  0.36 s to 0.16 s. Python 3.13 takes 0.064 s to 0.084 s on the same host.

## Follow-Up

- Typed scalar and string operands for `in`, `==`, and `get`.
- `aura_direct_vec_len` is now 13% of `int_list_sum`, because the bounds
  guard reads the length on every element. A `for` loop already knows its
  stop, so the guard could skip that call there.
- Lock removal still needs the retagging design change.
