# Batch 7 Step 8d, PR 3 of 4: Str, Queues, and Tasks

## Goal

Resolve `str`, `Queue`, `Task`, and `TaskGroup` member calls in MIR, so
the validator, the interpreter, and the emitter read one decision instead
of matching member names again. This is the third of four PRs for step 8d
of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
It builds on the `RuntimeOp` infrastructure from PR 1 and PR 2.

## Work Completed

- `RuntimeOp` has four new families. It is still `Copy`.
  - `Str(StrOp)` covers 22 `str` methods. The nine text-convenience
    methods, such as `trim_start`, `split_once`, and `repeat`, call the
    shared text dispatcher. Each one passes its dispatcher opcode in
    `runtime_call().immediates`.
  - `Queue(QueueOp)` covers `put`, `try_put`, `get`, `poll`, `get_or`, and
    `close`. It also covers the two private receives that a `for` loop over
    a queue lowers to, `__get_in_task_group` and
    `__get_with_registered_producers`.
  - `Task(TaskOp)` covers `result`, `poll`, and `result_or`.
  - `TaskGroup(TaskGroupOp)` covers `cancel`. Starting a task lowers to
    `StartTask`, so `cancel` is the one member call.
- `Queue.put`, `get`, `poll`, and `get_or`, and `Task.result`, `poll`, and
  `result_or` each have a `Timeout` form, such as `QueueOp::GetTimeout`, which calls the
  helper that takes a `Duration`. Lowering picks the timeout form when the
  call binds `timeout`, by name or by position. The base form has no
  `timeout` parameter, and the timeout form requires one.
- `RuntimeOp::with_timeout()` and `unspecialized()` map between the forms
  for every family. One function, `specialized_timeout_op`, picks the form
  for queue and task calls and for `wait_any` and `wait_all`.
- `RuntimeOp::resolve_member` resolves a private queue receive by its MIR
  spelling, only on a `Queue` receiver, and under the same class
  precedence as a public member.
- `RuntimeOp::result_type` covers the new families. Their entries are gone
  from `builtin_runtime_member_return_type` in `mir.rs`, and `Queue.get_or`
  is gone from `runtime_member_result_type` in `mir_local_types.rs`. The
  `str`, `list`, and `Array` slice entry stays for PR 4.
- The shared validator rejects a forged op of the new families: an op
  that does not apply to its receiver's type, a timeout form its
  arguments do not match, and a wrong arity, as for the other families.
- The interpreter forwards these ops to its existing evaluators by name. A
  `str` length on a place is measured where the string lies, as it was for
  the member spelling.
- The emitter calls these ops from the table. `str`, queue, task, and task
  group calls share the handle op path from PR 1, with a new argument
  layout for a queue payload and a boxed result for `get_or` and
  `result_or`. The text-convenience methods have their own small path,
  because the dispatcher takes its opcode first and fills unused argument
  words with zero.
- The emitter's `str` arms, except `__slice`, and its `Queue`, `Task`, and
  `TaskGroup` arms are gone. So are two arms nothing lowers to: `str.add`
  and `TaskGroup.close`. `with` cleanup still closes a task group inline.
  `native_codegen.rs` went from 16,181 to 15,603 lines.
- `is_builtin_wrapper` checks the module name with
  `is_builtin_root_module` instead of building the module's namespace, as
  in commit `14bcc42`.

## Verification

- New tests:
  - every `str`, queue, and task helper a member call makes has an op, and
    every op round-trips through its member or its private spelling;
  - each text method passes its dispatcher opcode;
  - every timeout form maps to its base form, takes a required `timeout`,
    and names the same member;
  - `str`, queue, task, and task group calls lower to ops, the pass is
    idempotent, and the result validates;
  - a module that spells these ops as member calls runs the same in
    memory, from serialized MIR, and in the emitter;
  - the validator rejects each forged op above at every public boundary;
  - `runtime_ops_tests.rs` checks result types, params, private receives,
    and display names.
- Tests that forged emitter arity errors on `str`, queue, and task calls
  now expect the validator's message. Tests that forged `str.add` and
  `TaskGroup.close` now expect an unknown member.
- 626 of 627 run-pass fixtures emit byte-identical objects. In
  `task_group_stack_override`, lowering now knows the result type of
  `Queue.get_or`, so `first = values.get_or(-1)` infers `int32`. Its
  comparison temporaries are typed `bool` instead of `Unit`, and `first`
  and `second` are copied into temporaries before each use, as other
  `Copy` locals are. The program prints the same output.
- Format, clippy, the hygiene check, the compiler and CLI suites, the
  backend parity matrix, and the coverage gate pass.

## Follow-Up

- PR 4 moves collections, `Array.clone`, spans, slices, and the remaining
  specializations.
- The interpreter still forwards ops by name. A later PR can give each op
  its own evaluator.
- The `Supervisor.start` argument defaults are still in the emitter.
