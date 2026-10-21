# Batch 7 Step 8d, PR 2 of 4: Named Calls, Print, and F-Strings

## Goal

Resolve named built-in calls, `print`, f-string part kinds, and host
builtins in MIR, so the validator, the interpreter, and the emitter read
one decision instead of matching call names again. This is the second of
four PRs for step 8d of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
It builds on the `RuntimeOp` infrastructure from PR 1.

## Work Completed

- `RuntimeOp` has three new families. It is still `Copy`.
  - `Function(FunctionOp)` covers 75 named built-ins: `range`, `sleep`,
    `select`, `wait_any`, `abs`, `parse_int64`, the `Array`, `Duration`,
    and collection constructors, `random::Rng`, and the `io`, `fs`,
    `process`, `net`, and `sys` module functions that have their own
    helper. `WaitAnyTimeout` and `WaitAllTimeout` are specializations
    for a call that passes a timeout.
  - `Print(PrintKind)` has six kinds: `Int`, `Uint64`, `Float32`,
    `Float64`, `Bool`, and `Value`.
  - `Host(HostBuiltin)` is an index into the runtime's host builtin table.
- `FunctionOp` and `PrintKind` serialize by variant name. `HostBuiltin`
  serializes as its qualified name, such as `json::parse`, so serialized
  MIR does not depend on the table's order. An unknown name fails to
  deserialize.
- The reachability walk keeps the builtin module wrapper that a named op
  calls. It reads op names with a typed walk over `Runtime` calls that
  have no receiver, after the existing string walk.
- `builtin_function_for_name` in `runtime_ops.rs` is the one precedence
  helper. `print` comes first, then a `FunctionOp`, then a host builtin. A
  user function of the same name wins over all three. The
  lowering pass, the validator, the interpreter, and the local type pass
  all call it.
- `sys::monotonic_time_ms` has both a dedicated helper and a host table
  entry. The helper wins, and `HostBuiltin` does not list the entry.
- Lowering decides each specialization. A `print` kind comes from the
  argument's type. `wait_any` and `wait_all` take the timeout form when
  they bind a timeout. An f-string part of type `int64` becomes the new
  `MirFormatPart::Int64`.
- `RuntimeOp::result_type(receiver, args)` is the one result-type table
  for every family. The duplicate arms for `range`, `parse_*`, `abs`,
  `min`, `max`, `round`, `divmod`, and `TaskGroup` are gone from
  `infer_expr_type` and `completed_call_type`.
- `runtime_target()` matches every op. It names a helper call, or a host
  builtin's own entry. `runtime_call()` keeps its PR 1 contract for ops
  that call a helper. `RuntimeCall` has a new `layout_words` field for the
  words the backend derives from layout, such as an `Array` dtype code.
- The shared validator rejects a forged named op:
  - an op whose name does not resolve to it, such as an op for a name a
    user function shadows;
  - an op with a receiver;
  - a `print` kind or timeout form its arguments do not match;
  - a wrong arity, an unknown argument name, or an argument bound twice;
  - a `select` with no sources or a named source;
  - an `Int64` f-string part whose operand is not `int64`.
- `run_mir` now resolves a hand-built module before it validates, as the
  emitter does. Both boundaries see the same form and give the same
  reason.
- The `Array` contract table in `mir.rs` keeps its operand, writeback, and
  result checks. Arity and argument names are now checked by the op
  validator, which runs first, so their duplicate branches are gone.
- The interpreter dispatches a `Runtime` call with no receiver by its op.
  It forwards to the existing evaluators, which moved into
  `evaluate_named_builtin`. A user function named like a builtin now runs
  as the user function.
- The emitter's `compile_named_call` now handles only user functions.
  `compile_named_runtime_op` dispatches on the op. Two small tables say
  how each `FunctionOp` passes its arguments and returns its result. The
  639-line builtin section, the host builtin and `io` call paths, and
  `lower_optional_opaque_arg` are gone. Arity checks the shared validator
  now makes are gone from the emitter's `range`, `select`, and user call
  paths. `native_codegen.rs` went from 16,786 to 16,197 lines.
- A user function named `list`, `dict`, `set`, or `Queue` now compiles.
  Lowering used to give its call no result type, so the validator
  rejected the program as an untyped local. The interpreter's check for
  task use now skips only real builtin wrappers, so such a function may
  start tasks.

## Verification

- New tests:
  - every named call helper has an op, every host entry but the replaced
    one has an op, and every op passes the arity its helper declares;
  - every op round-trips through its name and through serde;
  - named calls, `print` kinds, and `int64` f-string parts lower to ops,
    the pass is idempotent, and the result validates;
  - `wait_any` with a timeout takes the timeout form;
  - user functions named `set` and `Queue` keep their `Name` calls;
  - the validator rejects each forged op above at every public boundary;
  - a module that spells named ops as `Name` calls runs the same in
    memory, from serialized MIR, and in the emitter;
  - `runtime_ops_tests.rs` checks precedence and shadowing, each print
    kind, every `FunctionOp` result branch, params, specializations, host
    builtin names, and display names;
  - the emitter's own guards for a forged op that skips the validator, and
    the interpreter's error for a built-in it does not implement;
  - a new run-pass fixture, `user_functions_named_like_constructors`.
- Tests that forged emitter arity errors on named calls now expect the
  validator's message. Tests that find a named call in MIR JSON accept
  either spelling.
- All 626 run-pass fixtures that HEAD emits give byte-identical objects.
  The new fixture does not build at HEAD.
- Format, clippy, the hygiene check, the compiler and CLI suites, the
  backend parity matrix, and the coverage gate pass.

## Follow-Up

- PR 3 moves `str`, `Queue`, `Task`, and `TaskGroup` members.
- PR 4 moves collections, `Array.clone`, spans, and the remaining
  specializations.
- The interpreter still forwards named ops by name. A later PR can give
  each op its own evaluator.
- The `Supervisor.start` argument defaults are still in the emitter.
