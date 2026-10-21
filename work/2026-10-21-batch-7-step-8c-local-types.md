# Batch 7 Step 8c: Complete Local Types in MIR

## Goal

Record the type of every assigned MIR local in MIR itself, so no emitter
has to guess. This is step 8c of the [Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
The LLVM emitter in step 9 reads the same types.

## Work Completed

- Every `Instruction::Assign` whose target has no `.` now names either a
  parameter or a `local_types` entry with no `Unknown` in it.
- `crates/aura-compiler/src/mir_local_types.rs` is a new pass that runs in
  `Lowerer::finish`. It finds each assigned local that has no complete type
  and derives one from its rvalue. It repeats until no type changes, so a
  temporary can take its type from an earlier temporary.
- The pass reads facts lowering already has: class fields and methods,
  trait bounds, enum payloads, host builtin metadata, and builtin runtime
  members. A small table covers the runtime members of `Duration`,
  `random.Rng`, `Queue`, and the `process` types.
- Lowering now gives two kinds of expressions a better expected type. A
  slice of an untyped list literal takes the type it is assigned to. A
  builtin member argument such as `", ".join([])` takes the parameter type
  of the member.
- `type_implements_trait_bound` accepts a union when every member meets
  the bound, as ADR-0052 A8 states. Trait calls on such receivers now
  resolve.
- The shared validator `validate_loan_flow` rejects an assigned local with
  no type or with an `Unknown` type. The interpreter and the direct emitter
  both run it.
- The direct emitter no longer infers types. `infer_rvalue_type` and the
  helpers only it used are gone. Both places that used it now read
  `local_types` and report an internal error when a type is missing.
  `native_codegen.rs` went from 20,201 to 18,642 lines.
- MIR tests pin the types of the cases the emitter used to infer: print
  results, trait results, trait results on bounded generics, `Duration`
  members, and enum payloads. A validator test checks that an untyped or
  `Unknown` local is rejected.
- Tests that built MIR by hand without temporary types now either add the
  types or expect the validator to reject the module.

## Verification

- Every `.au` file in the repository that lowers passes the new validator
  check. That is 1,756 of 3,001 files. The rest are check-fail, parse-fail,
  and similar fixtures that do not lower by design.
- 615 of 626 run-pass fixtures emit byte-identical objects. The other 11
  improve:
  - `bound_method_generic_specialized`
  - `element_contextual_mutable_receivers`
  - `function_values_task_targets`
  - `generic_trait_bounds`
  - `multiple_trait_bounds`
  - `pythonic_convenience_methods`
  - `supertrait_bound_inherits_methods`
  - `supertrait_default_methods`
  - `task_group_stack_override`
  - `trait_mut_generic_receiver`
  - `trait_mutable_method_bounded_generic`
- In those 11, the emitter used to type some temporaries as opaque values.
  These were trait calls on bounded generic receivers and `Queue.get_or`.
  MIR now gives them their real types, such as `int64`, `int32`, `str`,
  and `None`. The objects box fewer values and do native integer math
  with overflow checks in place of runtime calls.
- An object dump taken with the old emitter and the new MIR matched the
  dump from the new emitter. Removing the emitter inference changed no
  bytes.
- Clippy, the compiler and CLI suites, and the backend parity matrix pass.

## Follow-Up

- Step 8d resolves runtime operations in MIR.
- The new runtime member table repeats part of
  `builtin_runtime_member_return_type`. Step 8d can merge them when it
  moves runtime operations into MIR.
- An enum variant temporary has the bare enum name, such as `Lookup`, with
  no type arguments. This matches what the emitter used before.
