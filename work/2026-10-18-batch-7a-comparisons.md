# Batch 7a Step 7: Unboxed Comparisons

## Goal

Remove the boxes a native comparison builds: the boxed `bool` result, the
boxed string literal, and the type tag written on every `str` read. This is
the rest of section E in the
[Batch 7a design checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md).

## Work Completed

- `aura_direct_compare_values(op, left, right, line, column)` returns a
  comparison of two runtime values as 0 or 1. The native code generator uses
  it for `==`, `!=`, `<`, `<=`, `>`, and `>=` on opaque operands.
- `aura_direct_compare_str_literal` compares a `str` value with a literal
  in the program image. It compares bytes, so the literal is never boxed or
  decoded. A non-`str` operand falls back to the general comparison.
- A runtime type tag for `str`, `bool`, `float32`, `float64`, or `Duration`
  only records the name. The runtime skips the value lock for those tags.
  Codegen skips the retag when a value keeps the same static leaf type.
- Fixture `run-pass/string_literal_comparisons.au` covers literals on both
  sides, a union operand, and indexed operands.

## Verification

- Runtime tests `direct_comparisons_return_unboxed_bits` and
  `leaf_runtime_type_tags_skip_the_value_lock`, and two new error cases.
- `cargo test -p aura --test backend_parity --test native_codegen_acceptance
  --test representation_stats`, and `cargo test -p aura-compiler --test fixtures`.
- Callgrind on `string_list_scan`: 3.76 G to 1.43 G instructions.

## Follow-Up

- Each list element read still clones the element into a fresh box.
