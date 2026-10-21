# Batch 7 Step 8d, PR 1 of 4: Resolved Runtime Operations

## Goal

Resolve built-in runtime operations in MIR, so the validator, the
interpreter, and the emitters read one decision instead of each matching
member names again. This is the first of four PRs for step 8d of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
It adds the infrastructure and moves resource handles, `random.Rng`,
scalar members, and value members.

## Work Completed

- `crates/aura-compiler/src/runtime_ops.rs` is new. It defines `RuntimeOp`
  with four families: `Resource` (fs, process, and net handles), `Rng`,
  `Scalar` (wrapping and saturating integer math, `to_float`, `sqrt`, and
  `Duration` conversions), and `Value` (`to_string`, `str.clone`, and
  collection `copy`).
- Each op names its member with `member_name()`, its runtime helper and
  immediate words with `runtime_call()`, and its result type with
  `result_type()`. `runtime_call()` matches every op, so a new op cannot
  compile without a helper.
- `result_type()` is the one result-type table for these ops. Their
  entries are gone from `builtin_runtime_member_return_type` in `mir.rs`
  and from `runtime_member_result_type` in `mir_local_types.rs`.
- MIR has a new call target, `CallTarget::Runtime { op, receiver,
  receiver_place }`. `CallTarget::member_parts()` gives the validator one
  view of `Member`, `TraitMember`, and `Runtime` receivers.
- `resolve_runtime_ops` in `mir_runtime_ops.rs` rewrites a member call to
  `Runtime` when its receiver has a concrete built-in type. It runs after
  `specialize_type_param_calls`, when the interpreter loads serialized
  MIR, and at the native emitter entry. It is idempotent, so hand-built
  MIR that still spells these calls as members works everywhere.
- A receiver whose type is a type parameter, a union, or a class that
  declares the method keeps its `Member` call. One precedence helper,
  `builtin_member_for_receiver`, decides this for the pass, the validator,
  and the interpreter.
- The shared validator rejects a forged op: an op that does not apply to
  its receiver's type, a missing receiver, an unknown argument name, an
  argument bound twice, too many arguments, or a missing required
  argument. Its arity messages use the emitter's old wording, such as
  ``expected `read_all()` to take no arguments``. The interpreter and the
  direct emitter now both report them, not only the emitter.
- The interpreter has a `Runtime` arm. It forwards to the existing
  receiver evaluators by `op.member_name()`. Those evaluators moved into
  `evaluate_receiver_method` so both arms share them.
- The native emitter calls these ops from the table. The string arms for
  them are gone. `native_codegen.rs` went from 18,642 to 16,786 lines.

## Verification

- New tests:
  - every op calls a declared runtime helper with matching arity, and each
    resource op has its own helper;
  - every op round-trips through its `BuiltinMember` and through serde;
  - file, socket, Rng, scalar, and value calls lower to `Runtime`, the
    pass is idempotent, and the result validates;
  - class methods named like built-ins, and calls on a type-parameter
    receiver, keep their member calls;
  - the validator rejects forged ops at every public boundary;
  - the interpreter gives the same output for resolved ops and for member
    spellings, and loading serialized MIR resolves member spellings again;
  - `runtime_ops_tests.rs` checks every op's result type, receiver
    resolution, class shadowing, and display name;
  - `mir_local_types_tests.rs` checks the arms left in
    `runtime_member_result_type` and the builtin argument table.
- Tests that forged emitter arity errors on hand-built member calls now
  expect the validator's message. Two cases forged `str.to_string()`,
  which the checker never accepts; the emitter now reports it as an
  unknown member. The JSON forging helpers in `batch1_coverage_native.rs`
  find a member call in either spelling, so the receiver-place test still
  covers `shuffle`, `to_float`, and `to_ms`.
- 623 of 626 run-pass fixtures emit byte-identical objects. The other 3
  differ for one reason:
  - `optional_api_child_pipes_absent`
  - `optional_flow_supervisor_qualified_arm`
  - `pythonic_convenience_methods`
- In those 3, lowering now knows the result type of
  `process.Child.wait_ok`, `process.Supervisor.start`, and similar calls,
  because the op table has them and the old builtin table did not. A
  `try` on such a result now moves the temporary instead of copying it
  (`MovePlace` in place of `Place`). The program behaves the same.
- Format, clippy, the compiler and CLI suites, the backend parity matrix,
  and the hygiene check pass.

## Follow-Up

- PR 2 moves named built-in calls, `print` kinds, f-string part kinds, and
  host builtins.
- PR 3 moves `str`, `Queue`, `Task`, and `TaskGroup`.
- PR 4 moves collections, `Array.clone`, spans, and the remaining
  specializations.
- `with` cleanup still names its exit method as a string, so the emitter
  resolves the op from that string at the cleanup site.
- The `Supervisor.start` argument defaults are still in the emitter.
