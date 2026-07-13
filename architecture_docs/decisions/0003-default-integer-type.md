# ADR-0003: Default integer type

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D3

## Decision

`int` aliases `int64`; the two spellings have one canonical type. Unsuffixed
integer literals default to `int64` only when no expected integer type is
available. Explicit fixed-width contracts remain fixed: a literal used in an
`int32` annotation, argument, return, field, collection, or generic context
adopts `int32` and must fit it. Contextual typing applies to the literal
expression; it does not implicitly narrow an already-bound `int64` value.

Overflow remains checked and trapping. Direct-backend `int64` and `uint64`
unboxing is a hard prerequisite for the default flip.

## Completion tests

- `crates/aurora-compiler/tests/fixtures/run-pass/default_integer_is_int64.au`
  and
  `crates/aurora-compiler/tests/fixtures/run-pass/default_integer_above_int32_succeeds.au`
  pin the new fallback type and range.
- `crates/aurora-compiler/tests/fixtures/run-pass/contextual_int32_literals_remain_int32.au`
  pins expected-type propagation through annotations, calls, collections,
  generics, and enums.
- `default_integer_generic_dispatch.au`,
  `generic_numeric_receiver_dispatch.au`, and
  `nested_numeric_generic_dispatch.au` pin numeric width through generic
  arguments, receivers, class fields, and enum payloads.
- `try_numeric_error_conversion_width.au` pins the statically declared source
  error width used by `try` and `From` conversion.
- `crates/aurora-compiler/tests/fixtures/check-fail/default_integer_above_int64_rejected.au`
  and its exact diagnostic pin checked overflow at the new default boundary.
- `parser_tests.rs::d3_parser_accepts_int_alias_as_numeric_cast_target` pins
  the alias in cast position.
- `sema_tests.rs::d3_int_alias_canonicalizes_across_signatures_generics_and_casts`,
  `d3_unhinted_integer_literals_default_to_checked_int64`, and
  `d3_int_alias_is_a_reserved_builtin_type_name` pin canonicalization,
  inference, bounds, and name reservation.
- `integer_tests.rs::d3_negative_literal_default_is_int64_and_does_not_widen_implicitly`
  pins the signed lower boundary and rejects implicit widening.
- The range, generic-conflict, and Vec-index check-fail fixtures pin that a
  defaulted `int64` variable is not silently narrowed into a fixed `int32`
  contract; the Vec semantic matrix covers read, write, `get`, `set`,
  `remove`, `swap`, and `insert`.
- `analysis_tests.rs::d3_analysis_reports_canonical_int64_for_aliases_and_defaulted_expressions`
  and
  `mir_tests.rs::d3_mir_canonicalizes_int_and_defaults_unhinted_integer_values_to_int64`
  pin editor-visible and lowered canonical types.
- `native_codegen_tests.rs::d3_native_unhinted_integer_operands_use_the_unboxed_int64_path`
  pins the defaulted scalar fast path.
- `ticket9_direct_backend_emits_unboxed_int64_ten_million_loop`,
  `ticket9_int64_and_uint64_are_unboxed_i64_direct_scalars`, and
  `direct_int64_unbox_helper_preserves_the_full_signed_range` guard the direct
  scalar prerequisite.
- `crates/aura/tests/backend_parity.rs` forces run fixtures through MIR and
  direct backends. The focused CLI parity matrix separately pins boundaries,
  aliases and casts; contextual `int32`; scalar generic dispatch; nested and
  receiver generic dispatch; `try` conversion width; and the uint64 negation
  failure path.
- Recovery completion and VS Code grammar tests pin editor recognition of
  `int`; `scripts/check-reference.sh` rejects the retired `int32`-default and
  no-bare-`int` claims.
- The Manual numeric/type chapters, tutorials, and proposal source plus HTML
  copy state the new default while retaining fixed `int32` API signatures.
