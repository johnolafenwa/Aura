# Batch 7 Step 8d, PR 4 of 4: Collections, Slices, and Spans

## Goal

Resolve `list`, `dict`, `set`, and `Array` member calls in MIR, including
the private reads, writes, slices, and span operations that indexing and
iteration lower to. The validator, the interpreter, and the emitter then
read one decision instead of matching member names again. This is the last
of four PRs for step 8d of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
It builds on the `RuntimeOp` infrastructure from PR 1 to PR 3.

## Work Completed

- `RuntimeOp` has five new families. It is still `Copy`.
  - `List(ListOp)` covers the 16 `list` methods, the checked read
    `__index`, the checked write `__set_index`, the slice `__slice`, and
    the iteration reads `__index_option` and `__take_index_option`.
  - `Dict(DictOp)` covers the 12 `dict` methods, the private
    `contains_key` and `set` that `in` and literals lower to, and
    `__index` and `__set_index`.
  - `Set(SetOp)` covers the 8 `set` methods and the two iteration reads.
  - `Array(ArrayOp)` covers the 17 `Array` methods, including `clone`, and
    `__index`, `__set_index`, and `__slice`. An `Array` has no iteration
    read, so `__index_option` does not resolve on it.
  - `Span(SpanOp)` covers the ten `__span_*` range operations on a list.
    Each op's opcode is its index in `SPAN_OPERATIONS`.
- `str` gains `StrOp::Slice` for `__slice`.
- Lowering picks a specialization from the receiver's element type and the
  arguments, in `specialized_member_op`:
  - `ListOp::PushScalar(kind)` and `ListOp::IndexScalar(kind)` for a list
    of `int64`, `float64`, or `bool`, which the runtime stores unboxed;
  - `DictOp::GetStr` for a `get` with a literal key on a dict with `str`
    keys;
  - `SetOp::ContainsInt64` for `contains` on a set of `int64`.
  The validator rejects an op whose specialization does not match its
  receiver and arguments.
- Each op names its helper and immediate words in `runtime_call()`. The
  scalar kind, the shared collection operation's opcode, the `Array`
  reduction kind, the `Array` arithmetic operation and mode, and the span
  opcode are immediates now, not emitter constants.
- `RuntimeOp::result_type` covers the new families. Their entries are gone
  from `builtin_runtime_member_return_type` in `mir.rs`, which keeps only
  `list.sort` and `list.filter`, the `with` cleanup closes, and the task
  group starts. The slice entry and `runtime_member_result_type` are gone
  from `mir_local_types.rs`.
- The interpreter forwards these ops to its existing member evaluators by
  op name. A length or slice of a place is still read where the value
  lies, and an `Array` method still runs on its place.
- The emitter calls these ops through one function per family. The op
  names the helper and its immediates, and the function lays out the
  arguments and the result. A member call the emitter resolves itself is
  specialized the same way. The emitter keeps its inline list length,
  inline scalar list and `Array` reads, boxing, coercions, and
  receiver store-backs.
- Three peepholes are keyed on resolved ops now: the bounds-check proof
  of `IndexInRange`, the in-place `len` and `get` of a lent list element,
  and the refusal to lend a receiver to a consuming read or a span op.
- The emitter no longer chooses a collection helper by member name.
  `native_codegen.rs` went from 15,603 to 14,862 lines.

## What Stays in the Emitter

- A scalar receiver's `add`, `sub`, `mul`, or `div` with an `Array` right
  operand still picks `ArrayBinary` with a scalar left operand by the
  operator's member name. No op covers an operator call on a scalar.
- `validate_rvalue` still checks the arity of an unresolved `__slice`
  member call before emission.
- `close` on an HTTP exchange, an HTTP response, or a WebSocket listener
  compiles to nothing, and `with` cleanup closes a task group inline.

## Verification

- New tests:
  - every `list`, `dict`, `set`, and `Array` helper a member call makes
    has an op, except the in-place element reads;
  - every private op round-trips through its MIR spelling on its receiver
    type;
  - each specialization is picked from its element type or literal key,
    and each op passes its kind, opcode, or layout words;
  - collection calls lower to specialized ops, the pass is idempotent, and
    the result validates;
  - a module that spells these ops as member calls runs the same in
    memory and from serialized MIR, and emits the same object;
  - the validator rejects a wrong specialization, a wrong receiver type,
    and a wrong arity at every public boundary;
  - the emitter refuses a forged op the validator did not check;
  - an unresolved member call is resolved and specialized in the emitter
    as lowering does it;
  - a lent `dict` element looked up with a key that is not a literal
    calls the boxed lookup.
- Tests that forged emitter arity errors on collection calls now expect
  the validator's message. Tests that matched `Member` calls on
  collections match `member_parts()` or the op instead.
- An `append` with no operand used to reach the element analysis, which
  poisoned every element. It now stops at the op's arity check, so that
  test expects the arity message at both boundaries.
- All 627 run-pass fixtures emit byte-identical objects.
- The run-fail fixtures and the proven-access tests pass unchanged.
- `aura run` takes the same time on a one-line program and runs a
  3,000-step list, dict, and set loop 6% faster.
- Format, clippy, the hygiene check, the compiler and CLI suites, the
  backend parity matrix, and the coverage gate pass. Six of the 1,412
  added lines that coverage measures are not run. Two are closing braces,
  two are the error arm of a `?`, and two are fallbacks for a receiver
  that is not a named type.

## Follow-Up

- Step 8d is done. Every runtime member call that has a helper is a
  resolved op.
- The interpreter still forwards ops by name. A later PR can give each op
  its own evaluator.
- The element analysis in `mir.rs` still matches mutating container
  members by name. It can key on ops instead.
- The scalar-left `Array` arithmetic choice above can become an op when
  operator calls on scalars resolve.
