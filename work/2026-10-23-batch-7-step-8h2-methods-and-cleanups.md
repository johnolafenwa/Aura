# Batch 7 Step 8h Part 2: Method Calls and Cleanups in MIR

## Goal

Finish step 8h of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
Part 1 bound runtime op arguments and recorded generic call type
arguments. This part moves the last semantic items of the 8g inventory
into MIR:

- method resolution and dispatch, 850 lines at `ff35ee70`: class methods
  by name, trait impls by specificity, union and erased receivers, `clone`
  on a union, and the `From` impl behind `try`;
- the `with` cleanup exit method, named as a string, and the inline
  `TaskGroup` close, 147 lines;
- `container_mutation_callables` in the MIR validator, which matched
  mutating members by name.

Objects stay byte-identical to part 1, `ecda8853`.

## Design

### Member calls

- `CallTarget::Member` and `CallTarget::TraitMember` gain `dispatch`, a
  `MethodDispatch`. Serde skips it when absent, so older MIR still reads.
- `MethodDispatch` has six forms:
  - `Static { function }` calls one method with the receiver first. A
    class's own method wins, then the most specific trait method for the
    receiver's type, then for the class. A `TraitMember` call looks only at
    its trait.
  - `Runtime(op)` is a runtime op on one member of a union receiver.
  - `Nothing` is `close` on a handle that owns no host resource.
  - `Union(arms)` holds one arm for each member of a union receiver, in
    member order.
  - `UnionClone` is `clone` on a union.
  - `Candidates { candidates, erased }` lists the types that declare the
    method, most specific first, each with the function it calls. The
    receiver's runtime type picks one. `erased` marks a type parameter
    receiver, whose union value is matched by its active member.
- `mir::methods::resolve_methods` fills the dispatch from the receiver's
  MIR type. It runs after the ownership pass, in `lower` and at the native
  boundary for an implicit module. `lower_implicit` skips it, so `aura run`
  pays nothing.
- The emitter's `compile_dispatch` reads the dispatch. It no longer looks
  up a class method, a trait impl, or a candidate list.

### `try` conversions

- `Rvalue::Try` gains `from`, the `from` method of the most specific `From`
  impl, when the operand's error type differs from the function's. The
  emitter calls it.

### Cleanup callees

- `Instruction::PushCleanup` gains `callee`, a `CleanupCallee`:
  `Method { function }` for a class's `close` or `__exit__`, `Runtime(op)`
  for a built-in resource's `close` op, `CloseTaskGroup`, or `Nothing` for
  a handle that owns no host resource.
- The cleanup thunk and the inline cleanup read the callee. The thunk still
  picks a typed close entry from the place's static type, which is
  translation.

### Validation

- `validate_methods` resolves each call, `try`, and cleanup in a reachable
  block again. It refuses one that names a callee its types do not resolve
  to. It runs at the native boundary, and in the interpreter for MIR with
  explicit ownership.
- The native boundary also refuses a call that names no callee. The
  interpreter accepts an unnamed call that resolves to nothing, because it
  dispatches on the runtime value. The generic body of the builtin
  `json::encode` makes such a call.
- Hand-built forged MIR now fails in the validator, where it used to fail
  in the emitter. The tests that check those messages follow the new text.

### Container mutations

- `container_mutation_callables` matched `append`, `clear`, `insert`,
  `pop`, `sort`, and the other mutating names on any member call. A class
  with a method of such a name and a field that holds a function lost the
  field's callable identity after the method call. Both backends then
  refused the next call of the field with `no authoritative callable
  contract`.
- It now reads only a `CallTarget::Runtime` call, by its op. The new
  fixture `user_methods_named_like_container_mutations` covers it. It
  failed on both backends before the change.

### Borrowed-element fusion

- The fusion helper choice stays in the emitter for step 9. It is an
  instruction choice with the same result.
- Its guard looked up a trait method of the element's type by the member
  name. MIR already resolved the call to the built-in op, which runs
  whatever the trait impls declare, so the guard is gone.

## Work Completed

- `mir.rs` adds `MethodDispatch`, `MethodCandidate`, `CleanupCallee`, the
  `dispatch`, `from`, and `callee` fields, and the pass and validator calls.
- `mir_methods.rs` and `mir_methods_tests.rs` are new.
- `native_codegen.rs` loses `compile_trait_member_call`,
  `compile_member_call_on_receiver`, `compile_member_call_on_value`,
  `compile_opaque_member_call`, `union_member_call_result_type`,
  `dynamic_method_candidates`, `find_trait_method` and its variants,
  `find_concrete_impl_method`, `find_method`, `cleanup_exit_method`, and
  the `From` impl search. `compile_class_member_call` became
  `compile_method_call`.

## Verification

- Objects: all 751 run-pass and run-fail objects are byte-identical to
  `ecda8853`. The new fixture adds one object.
- Forged MIR: `mir_methods_tests.rs` checks that both boundaries refuse a
  call that names another method, a missing dispatch, a cleanup that names
  a callee other than its exit, and a `try` that names the wrong `From`.
- Parity: every run-pass and run-fail fixture, 788 in all, matches on both
  forced backends.
- Determinism: `direct_objects_are_identical_across_repeated_builds`
  passes.
- Unit tests: `aura-compiler` 1,663 library and 1,001 integration tests,
  `aura-runtime` 740, `aura` 501. `package_io_helpers_cover_local_error_paths`
  is skipped.
- `cargo fmt --check`, CI clippy, and `scripts/check-hygiene.sh` pass.
- Coverage: lines 96.51%, functions 97.37%, regions 95.46%. The floors are
  96.46%, 97.33%, and 95.23%.
- Startup: `aura run` of a one-line program runs 92.22M to 92.23M
  instructions under callgrind, against 92.22M to 92.24M at `ecda8853`.
- Lines: `native_codegen.rs` shrinks from 14,600 to 14,202.
  `native_codegen_callables.rs` (1,448) and
  `native_codegen_reachability.rs` (134) do not change. `mir_methods.rs`
  is 750 lines, and `mir.rs` grows from 26,892 to 27,019.

## Follow-Up

- The interpreter still resolves methods, `From` impls, and cleanups
  itself from runtime values. It ignores the MIR callees.
- `rebase_container_result_callables` in the loan validator still reads
  member names such as `get` and `pop` to rebase a container's callables
  onto a call's result.
