# Batch 7 Step 8h Part 1: Bound Arguments and Type Arguments in MIR

## Goal

Finish part 1 of step 8h of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
Step 8g found three semantic items in the emitter that this part moves
into MIR:

- named-argument binding for runtime ops, 144 lines, and the
  `Supervisor.start` defaults with the other `process` defaults, 71 lines;
- scalar-left `ArrayBinary` by member name, 48 lines;
- generic call type arguments, 166 lines.

Objects stay byte-identical to `c011df6a`. Part 2 moves method resolution
and cleanup callees.

## Design

### Runtime op arguments

- `resolve_runtime_ops` binds every `Runtime` call's arguments with
  `bind_runtime_op_args`. Named arguments bind by name and the rest in
  order, as the checker binds them. The bound form has one positional
  argument per parameter, in parameter order.
- An optional parameter the call leaves out gets `RuntimeOp::default_arg`.
  A default with a constant spelling is that constant: `None` for `cwd`
  and `true` for `group`. Any other default is an `Operand::Default`
  that names it, from `ArgDefault`: `EmptyStrDict`, `StdioNull`,
  `StdioInherit`, `RestartOnFailure`, `Milliseconds(100)`, `Int32(-1)`,
  and `Int64(-1)` for `pop`'s `index`. `Default(Omitted)` stands for a
  default the runtime helper applies itself, such as no deadline for an
  omitted `timeout`. A backend passes a zero word for it.
- `range` with one argument binds it to `stop`. `select` keeps its
  positional sources.
- The shared validator calls the same binder. It refuses a call that is
  not already bound: a missing, extra, repeated, unknown, or named
  argument, or a default that is not its parameter's. It refuses
  `Operand::Default` in any call that is not a runtime op.
- The emitter loads each `Default` operand. It no longer binds or orders
  arguments, and its arity checks became `require_bound_args`. The
  interpreter drops each `Default(Omitted)` argument and names the rest
  after their parameters, so its built-in evaluators run as before.
  `Supervisor.start` takes every argument as bound.
- The ownership pass reads a runtime op argument's role by position only.

### Number methods with an `Array` argument

- The emitter turned `factor.mul(values)` into the `Array` operator when
  `factor` was a number and `values` an `Array`, by member name. It did
  so even when the program implemented `mul` on the number's type, and
  the interpreter then ran the program's method. The two backends printed
  different results.
- `resolve_runtime_ops` now rewrites such a call to `Rvalue::Binary` with
  trap `Runtime` when no trait impl for the receiver's type declares the
  method. A program's own method stays a `Member` call, and both backends
  run it. The new fixture `scalar_trait_method_with_array_argument` covers
  both forms.

### Generic call type arguments

- `Rvalue::Call` gains `type_args`, a map from each type parameter of a
  generic callee to the type it stands for at the call. Serde skips an
  empty map, so older MIR still reads.
- `mir::type_args::record_call_type_args` fills it after the ownership
  pass: in `lower`, and at the native boundary for an implicit module.
  It matches the callee's return type against the destination first, then
  each parameter against its argument, with the rules the emitter used.
  A type parameter that nothing fixes stands for itself. `lower_implicit`
  skips the pass, so `aura run` pays nothing for it.
- `validate_call_type_args` refuses type arguments that do not name
  exactly the callee's type parameters. It runs at the native boundary,
  and in the interpreter for MIR with explicit ownership.
- The emitter builds its substitutions from `type_args`. The binder moved
  to `mir_type_args.rs` with its unit test.

## Work Completed

- `runtime_ops.rs` adds `ArgDefault` and `RuntimeOp::default_arg`.
- `mir.rs` adds `Operand::Default`, the `type_args` field, and the
  `type_args` pass in `lower` and at the boundary.
- `mir_runtime_ops.rs` adds `bind_runtime_op_args`, the bind rewrite, and
  the scalar-left `Array` rewrite. `mir_runtime_ops_tests.rs` tests them.
- `mir_type_args.rs` and `mir_type_args_tests.rs` are new.
- `native_codegen.rs` loses `ordered_named_args`,
  `ordered_optional_named_args`, `required_named_arg`,
  `bound_runtime_op_args`, `bind_named_runtime_op_args`,
  `default_runtime_op_arg`, `collect_direct_runtime_type_substitutions`,
  and the scalar-left intercept. It shrinks from 14,942 to 14,600 lines.
- `mir_runtime.rs` evaluates `Default` operands and loses
  `bind_optional_builtin_args`.
- `aura run` and `aura test` no longer resolve runtime ops a second time.
  `lower_implicit` already resolved and bound them, and the pass is
  idempotent. The first version bound every call again in the
  interpreter's loader, and `aura run` then spent 1.4M more instructions.
  Hand-built MIR still goes through `resolve_runtime_ops` at the loader.

## Verification

- Objects: all 750 fixture objects, 123 of them run-fail, match
  `c011df6a` byte for byte. The new fixture adds one more. The
  determinism test passes.
- Parity: `backend_parity` passes with `--ignored`.
- Tests: `aura-compiler` lib 1,652 and integration 993 pass,
  `aura-runtime` 740, and `aura` 501.
- Forged MIR: both boundaries refuse a runtime op with a missing or extra
  argument, a wrong default, a default in a user call, and a call whose
  type arguments name the wrong type parameters.
- Lint: `cargo fmt --check`, the CI clippy command, and
  `scripts/check-hygiene.sh` pass.
- Coverage: lines 96.52 (floor 96.46), functions 97.38 (floor 97.33), and
  regions 95.44 (floor 95.23).
- Startup: `aura run` of a one-line program takes 92.22M to 92.24M
  instructions, against 93.34M to 93.67M at `c011df6a`.

## Follow-Up

- Part 2 moves method resolution and dispatch, and the `with` cleanup
  callees.
- The validator does not refuse a `Default` operand outside a call. Both
  backends report one they cannot load.
