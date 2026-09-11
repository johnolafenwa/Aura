# Backend boundary: checked semantics and mechanical emission

Status: pre-Batch-1 item 2 delivered for the 0.3.4 update. This is an inventory
and proposed design, not a refactor or a second backend. Cranelift remains the
native backend through Batch 6; MIR execution remains maintained.

## Reading the inventory

Line references below refer to the source in the pre-Batch-1 implementation;
the named function is the durable navigation anchor. Paths are relative to the
repository root. “Enough” asks whether checked MIR already carries the semantic
inputs, not whether it has an explicit operation ready for mechanical emission.
Representation choices such as register allocation stay in an emitter. Language
choices such as copying versus consuming, failure classification, and evaluation
order belong in shared lowering or a specified runtime operation.

## Native codegen inventory

All references in this table are in
[`crates/aura-compiler/src/native_codegen.rs`](../crates/aura-compiler/src/native_codegen.rs).

| Line and function | Observable semantic currently decided here | Category | Does checked MIR carry enough? |
| --- | --- | --- | --- |
| 236 `DirectType::abi_types`, 275 `field_slice` | Flattens class values and locates mutable field writeback slots; opaque versus scalar handling determines the ownership ABI. | typing, ownership | Types and field declarations exist; an explicit layout/writeback contract is missing. Keep target register placement native. |
| 319 `bind_function_value_args`, 766 `ordered_named_args` | Binds positional/named arguments and supplies the ordering used by indirect and builtin calls. | evaluation order | `MirArg`, parameters, and default-function metadata exist; represent bound argument slots once, preserving already-lowered source evaluation order. |
| 3516 `define_function_thunk`, 3690 `define_function_default_binder` | Adapts code pointers, default evaluation, mutable argument returns, and boxed results. | typing, ownership, evaluation order | Most inputs exist; unify a callable ABI plan rather than reconstructing capabilities in each emitter. |
| 3788 `define_cleanup_thunks`, 3805 `define_cleanup_thunk` | Selects cleanup calls and reconstructs resource arguments for scope/trap exits. | cleanup, runtime-call selection | `PushCleanup`/`PopCleanup` and types exist; a resolved cleanup target and explicit ownership of captured cleanup arguments are still needed. |
| 4091 `reachable_direct_block_labels`, 4137 `direct_view_selector_tags`, 4246 `direct_closure_writeback_live_ins` | Reconstructs reachable loan alternatives and live closure writebacks across control flow. | ownership | Loan instructions and CFG exist; lowering should publish a validated place/loan dataflow plan. Backend block layout must not decide provenance. |
| 4810 `temporary_owns_opaque`, 4835 `release_all_temporary_owned`, 4878 `transfer_opaque_arg`, 4900 `export_return_value` | Decides retain/release/transfer for temporaries, arguments, roots, and returns. | ownership, cleanup | Passing modes and types exist, but value liveness/ownership operations remain implicit. Add explicit value effects before moving emission. |
| 5047 `compile_instruction` | Chooses scheduling fuel and translates loan acquisition, projected write-through, returned projection selection, and cleanup registration. | ownership, cleanup, runtime-call selection | MIR names the semantic operations. Fuel is a backend cost policy; selected-place and cleanup behavior must be a shared contract. |
| 5303 `compile_terminator`, 5478 `emit_return_value` | Performs exit releases and return writeback while choosing branches, assertion traps, and match paths. | cleanup, trap classification | Terminators, spans, and captures exist; ordered exit effects are not fully explicit. |
| 5656 `compile_closure` | Builds capture storage and passes consuming/repeatable metadata to the runtime. | ownership, typing | `Rvalue::Closure`, capture modes, and signature exist. Resolve environment layout and capture transfer centrally. |
| 5822 `compile_unary`, 5886 `compile_wide_integer_negation`, 5912 `compile_cast` | Chooses checked negation, cast bounds, narrowing, float rounding, and boxed conversion routes. | typing, trap classification | Operand/target types exist; a shared typed numeric operation must select the exact check and diagnostic. |
| 6029 `compile_binary`, 6187 `compile_int32_binary`, 6244 `compile_signed_floor_divmod`, 6271 `compile_wide_integer_binary`, 6429 `compile_float_binary` | Distinguishes checked overflow, divisor-sign floor/remainder, float width, Array/scalar arithmetic, and fallback boxed arithmetic. | typing, trap classification, runtime-call selection | Operators, types, and spans exist. Explicit typed operations and trap policy are still missing. |
| 6537 `emit_int_division_guard`, 6547 `emit_integer_overflow_failure_branch`, 6577 `emit_float_division_guard` | Chooses when to trap and how to classify/location-tag the diagnostic. | trap classification | Source spans exist; diagnostic identity and failure edges should be resolved before native emission. |
| 6610 `compile_call`, 6641 `compile_extern_call`, 6753 `compile_function_value_call` | Selects direct/indirect/FFI calls and adapts owned versus mutable arguments and returns. | ownership, runtime-call selection | MIR distinguishes targets and FFI contracts. The concrete argument effect and writeback plan is still backend-derived. |
| 6929 `compile_print`, 6976 `compile_format_string` | Chooses scalar versus boxed rendering and formatting helpers. | runtime-call selection, evaluation order | Operands and formatting data exist; canonical rendering operation IDs would remove string-based dispatch. |
| 7027 `compile_named_call`, 7738 `compile_host_builtin_named_call`, 7908 `compile_builtin_io_named_call` | Selects builtin behavior, optional arguments, resource helpers, and result representations. | typing, runtime-call selection | Shared builtin metadata exists; resolve a typed builtin opcode and argument slots in MIR. |
| 8231 `compile_for_range`, 8285 `compile_trait_member_call`, 8317 `compile_member_call` | Chooses iteration behavior, trait dispatch, scalar/Array special cases, and receiver writeback. | ownership, typing, runtime-call selection | Checked types and contracts exist, but resolved calls should replace repeated member-name inference. |
| 8548 `compile_construct`, 8611 `type_of_place`, 8650 `load_operand` | Selects field layout, operand coercion, projection access, and copy/opaque loads. | typing, ownership | Nominal declarations and local types exist; publish typed place IDs and explicit load modes. |

## Native runtime inventory

All references in this table are in
[`crates/aura-compiler/src/native_runtime.rs`](../crates/aura-compiler/src/native_runtime.rs).
A runtime must still implement dynamic checks and host I/O. The goal is to make
those operations specified and shared, not to move operating-system execution
into compile time.

| Line and function | Observable semantic currently decided here | Category | Does checked MIR carry enough? |
| --- | --- | --- | --- |
| 739 `capture_direct_runtime_frames_once` | Captures innermost-first Aura call frames and task ancestry once for the primary diagnostic. | trap classification | Spans and call identities exist; task ancestry is necessarily dynamic. Specify one cross-backend frame contract. |
| 769 `release_direct_task_runtime_state`, 805 `push_direct_cleanup_registration`, 819 `take_direct_cleanup_registration`, 859 `DirectPrimaryDiagnosticGuard::install` | Releases task-owned values, orders registered cleanup, and preserves the first failure. | cleanup, ownership, trap classification | Scope actions exist; runtime registration/draining and failure precedence need a shared explicit contract. |
| 1132 `extract_duration_nanoseconds`, 1157 `direct_timer_diagnostic` | Converts Duration to host timer bounds and maps failure to Aura diagnostics. | typing, trap classification | Duration type is known; actual value/host bounds are dynamic. Shared conversion policy should be named by the operation. |
| 1173 `boxed_value_with_type`, 1188 `retain_ref_count`, 1206 `release_ref_count` | Registers boxed ownership and determines last-release destruction. | ownership, cleanup | MIR lacks explicit retains/releases today. Runtime representation stays here after ownership effects become explicit. |
| 1292 `canonical_runtime_type_name`, 1349 `runtime_type_from_name`, 1458 `runtime_type_pattern_matches` | Reconstructs nominal/generic identities for dynamic dispatch and value tagging. | typing | MIR carries types; use canonical type IDs/descriptors instead of independently interpreting strings. |
| 1981 `evaluate_direct_json_host_builtin` | Chooses exact JSON variants, accessor outcomes, consuming payload extraction, and parse/dump error policy. | typing, ownership, trap classification | Builtin target/types are known; codec results are dynamic. Centralize shared builtin semantics while retaining runtime codec work. |
| 2335 `direct_value_to_ffi`, 2399 `direct_ffi_to_value`, 2448 `direct_ffi_write_back_mut_bytes`, 2463 `direct_ffi_error` | Marshals scalar/view/handle arguments, copies mutable scratch back, and maps native adapter failures. | ownership, typing, trap classification | MIR has FFI signature/capabilities. Dynamic native results still require runtime validation under the same ABI contract. |
| 3934 `aura_direct_function_bind_defaults`, 4002 `aura_direct_function_call` | Applies default binders, claims consuming closure captures, reconstructs environments, and writes mutable captures back. | ownership, evaluation order | Static capture/call-kind metadata exists. Dynamic invocation and consumption checks stay runtime operations with specified effects. |
| 5705–5984 Array entrypoints | Determines runtime shape/index bounds, fresh allocation versus mutation, integer mode, callback traversal, and sequential reduction. | typing, ownership, trap classification | Dtype and operation exist; shape and indexes are dynamic. Shared Array helpers should remain canonical. |
| 6643 `aura_direct_binary_value`, 6684 `aura_direct_binary_value_at`, 6734 `aura_direct_cast_value` | Maps numeric opcode/width to value semantics and consumes boxed operands; emits typed traps. | ownership, typing, trap classification | Operators, widths, and spans exist. Resolve these into a typed runtime call without duplicating arithmetic rules. |
| 3024 `runtime_diagnostic_error`, 3073 `task_runtime_boundary` | Converts runtime failures to typed diagnostics and drains cleanup on unwinding, retaining primary failure and Aura frames. | cleanup, trap classification | Static spans exist; dynamic failure state requires a shared boundary contract. |
| 8326 `aura_direct_task_join`, 8676 `aura_direct_wait_all`, 8807 `aura_direct_select` | Claims task results, chooses timeout/cancel/error carriers, and validates heterogeneous result metadata. | ownership, typing, trap classification | Static result types exist; readiness/consumption are dynamic. Preserve one-observation semantics in shared runtime operations. |
| 8871 `aura_direct_task_group_close`, 8888 `aura_direct_close_value` | Decides cancellation-before-join and selects resource-specific close behavior, including ignored WebSocket close errors. | cleanup, runtime-call selection | `cancel_before_cleanup` exists; resolved resource close operation and failure policy should be explicit. |

## Proposed mechanical builder

Shared lowering would consume a checked `Program` and produce validated executable
MIR with typed values/places, bound calls, ownership effects, ordered exits,
explicit trap descriptors, and stable runtime operation IDs. A target-neutral ABI
plan would describe argument/result slots without choosing machine registers.
The emitter would implement a small builder that creates blocks, values, loads,
stores, calls, and terminators. It would not infer a receiver capability from a
method name or decide whether a return consumes an object.

This sketch is illustrative, not a committed API or new Rust implementation:

```rust
trait NativeBuilder {
    type Value;
    type Block;
    type Error;

    fn block(&mut self, id: BlockId) -> Self::Block;
    fn switch_to(&mut self, block: Self::Block);
    fn scalar(&mut self, op: ResolvedScalarOp, args: &[Self::Value])
        -> Result<Self::Value, Self::Error>;
    fn load(&mut self, place: &ResolvedPlace) -> Result<Self::Value, Self::Error>;
    fn store(&mut self, place: &ResolvedPlace, value: Self::Value)
        -> Result<(), Self::Error>;
    fn call(&mut self, call: &ResolvedCall, args: &[Self::Value])
        -> Result<Vec<Self::Value>, Self::Error>;
    fn effect(&mut self, effect: &ResolvedEffect, args: &[Self::Value])
        -> Result<(), Self::Error>;
    fn terminate(&mut self, exit: &ResolvedTerminator, args: &[Self::Value])
        -> Result<(), Self::Error>;
}
```

`ResolvedScalarOp` selects width and a previously specified arithmetic mode;
`ResolvedCall` names the exact function/runtime operation and ABI slots;
`ResolvedEffect` covers retain/release, loan transitions, cleanup registration,
and safepoints. Trap descriptors retain diagnostic code, source span, and frame
identity. The builder may optimize instruction selection, but cannot omit an
observable check or reorder an effect. Runtime implementations remain responsible
for dynamic allocation, readiness, host errors, and actual cleanup execution.

## Ordered Batch 1 moves and gates

Every move starts with a failing behavioral or MIR regression. Every move must
pass the complete forced MIR/direct fixture matrix (no fallback, loopback enabled),
compiler coverage floors **96.30% lines / 97.21% functions / 94.71% regions**, and
all affected CLI/native acceptance tests. No floor reduction or fixture rewriting
is permitted to accommodate a semantic change.

1. Introduce stable typed operation and runtime-call descriptors alongside the
   existing serialized MIR contract. Pin builtin argument binding/default order,
   numeric width, and diagnostic spans with MIR and public-interface tests.
2. Move numeric-operation and trap selection into shared lowering. Exercise
   integer extremes, floor division/remainder, float widths, casts, and Array
   modes on both backends; retain exact diagnostic comparisons.
3. Publish canonical place/projection and reachable loan plans. Regress returned
   view alternatives, reborrow suspension, branch expiry, and closure writeback
   before replacing codegen's reconstruction. Keep public/serialized MIR checks.
4. Make ownership effects and ordered exit actions explicit. Test every normal,
   early-return, trap, and cancellation exit, with exact-once cleanup and primary
   failure preservation. Both direct runtime tests and CLI frame tests are gates.
5. Resolve callable/default/receiver ABI plans for Batch 1's new callable types.
   Preserve left-to-right evaluation, bound names/defaults, mutable sinks, and
   consuming captures. Run compiler analysis/LSP regressions and 100% LSP coverage
   when semantic metadata changes; bump its schema when representation requires it.
6. Route Cranelift through the small builder after each operation family has a
   shared contract. Retain standalone packaged execution and source diagnostics;
   do not add another native backend or change the default execution mode.

The runtime/compiler crate split remains Batch 5. A second native backend is a
Batch 7 evidence-based decision under [ADR-0064](decisions/0064-native-backend-strategy-and-codegen-boundary.md).

## Per-call cost attribution

Pre-Batch-1 item 7a profiles the unchanged naive `fib30.au` input using Xcode
16.0 `xctrace` Time Profiler on the maintained arm64 Mac14,9. The compiler and
runtime are built with `CARGO_PROFILE_RELEASE_STRIP=none`; user linking uses
`AURA_NATIVE_KEEP_SYMBOLS=1`. Compiler source is
`fc0361bc76b8b47d300a3089b1b4bb7c2b739dfc`. All eleven recordings succeeded;
no `sample` fallback was needed.

The table assigns each recovered Fibonacci-stack sample to one category by
its leaf symbol and ASLR-normalized arm64 instruction address. Inlined depth
and arithmetic instructions are separated using the retained disassembly.

| Category | Samples | Share of recovered Fibonacci samples |
| --- | ---: | ---: |
| Emitted Aura function body | 48 | 3.42% |
| Runtime call-frame push and pop, including metadata validation | 691 | 49.29% |
| Recursion-depth accounting | 22 | 1.57% |
| Safepoint and backedge yield polling | 0 | 0.00% |
| Checked integer arithmetic | 29 | 2.07% |
| Argument and result marshalling | 88 | 6.28% |
| Other runtime | 524 | 37.38% |
| Unattributed within recovered Fibonacci stacks | 0 | 0.00% |

There are **1,402 recovered Fibonacci samples out of 1,502 process samples**.
The 100 excluded samples include startup/protocol work and ten rows with no
recovered backtrace; zero in the last row does not mean every process sample
was attributable. Sampling skid and optimized instruction scheduling limit
fine-grained attribution. The argument/result category counts scalar register
moves, not boxing. This non-tasking program has no emitted safepoint polling.
Other runtime includes task-key/TLS/state lookup, returned-view bookkeeping,
and runtime prologues/epilogues. Source frames and task ancestry remain intact;
this input creates no child-task ancestry.

Separate diagnostic timings use the unchanged READY/GO/DONE protocol, two
warmups and eleven alternating observations per lane without the profiler.
The original binary's median is **91.165375 ms**, or **33.85854 ns per logical
call** for `2 × F(31) − 1 = 2,692,537` calls. This divides whole recursive-work
elapsed time by the naive algorithm's call count; it is not isolated entry
latency or a new contractual release measurement.

A safe in-place `DirectCallFrameStorage::Spill` push/pop experiment measured
81.045542 ms, **30.10007 ns per logical call**, an **11.10% improvement**. It
failed the task's required 20% acceptance threshold and was reverted. Its patch
and paired observations are retained; no optional per-call optimization ships.
The unchanged fixtures and full parity gate remain the acceptance requirement
for any future adopted implementation.

Batch 7 inputs are:

- Metadata UTF-8 validation accounts for **400/1,402 samples (28.53%)**. Seek
  validation amortization that retains all current invalid-metadata diagnostics
  and lifetime guarantees. An unchecked exported helper is not an acceptable
  shortcut: runtime globals remain reachable through process-global FFI, and
  readable invalid UTF-8 must not become undefined behavior.
- Task-key and TLS helper leaves account for **257/1,402 samples (18.33%)**,
  before inline state-map lookup. Investigate repeated lookup while preserving
  task identity, nesting, worker pinning, frame capture and cancellation.
- Frame-storage movement has a measured **11.10%** candidate improvement,
  below this task's threshold. Revisit it as part of a broader measured design.
- Changes to the ADR-0036 frame contract, recursion-depth policy, safepoint
  placement or checked arithmetic require their own approved design. Their
  sampled shares are not permission to remove those semantics. A different
  emitter alone does not remove the dominant Rust-runtime work.

[Versioned evidence](https://github.com/johnolafenwa/Aura/tree/main/work/2026-09-08-pre-batch-1-items-6-7)
includes lossless raw sample XML, exact record/export commands, selected
instructions, an executable attribution script, binary hashes, the reverted
experiment and all paired timings. These are diagnostic quiet-host observations,
not post-reboot release results.

## Batch 1 phase 1: normalized types and alias expansion

The first type family establishes a shared producer in `sema/types.rs` for
flattened union members and canonical structural keys. Alias constructor and
member adaptation lives in `sema/aliases.rs` and uses checked `AliasInfo`
metadata in both semantic checking and MIR lowering. Imported implementation
bodies retain private alias metadata without exporting those names for lookup.
The direct emitter receives expanded nominal types; it does not resolve aliases
or choose a preferred union-member order. Checked lowering uses the same
bounded expansion with no duplicate charge to semantic aggregate accounting.

This establishes type identity inputs for the ordered moves below the original
inventory. Injection, explicit union layout/tag plans, callable ABI and binding
plans, and property dispatch are still pending their respective families; no
native emission decision is claimed moved merely because a type is accepted.

## Batch 1 phase 1: checked union injection

`sema/unions.rs` selects an injection member exactly once under the expected
type boundary. Literal probes isolate ownership, loan, obligation, and
expression metadata; shared compilation budgets still charge their work.
The resulting module/source-position/type identity record is retained with
imported bodies. MIR consumes it as `UnionInject`, including the canonical tag,
member type, union type, and payload operand. Conditional and match arms receive
their expected result context in shared lowering.

Both runtime paths execute this operation without another member search.
The common MIR validator checks the selected member, operand and destination
types before either backend executes. Immediate owned payload transfer uses
the existing tracked ownership ABI; it does not change frame or scheduler
contracts. Dense tag selection has moved into shared checking/lowering.
Borrowed injection builds real union storage for Copy snapshots and fresh
values; it rejects implicit cloning of non-Copy member places. Returned-view
origins require an exact union place. Common call validation checks union
operand storage for named, method, and indirect function-signature calls,
including metadata supplied through the public MIR interface.
Native storage layout, allocation-free union locals, generic tag remapping,
property dispatch, and callable layouts remain the later ordered families.

## Batch 1 phase 1: union type patterns

`sema/patterns.rs` now owns pattern checking, guard capabilities and coverage.
MIR emits `UnionTagTest` with a canonical type and member index, guarded
payload loans, and `UnionTakePayload` after an owned guard commits. The common
validator tracks true-edge tag facts through physical loan origins, intersects
facts at joins, invalidates them on mutation or move, and merges possibly
consumed storage at joins. Reinitialization begins a new storage generation.
Payload views cannot increase capability or escape through an arm-local return.

The interpreter rechecks active tags before projecting, writing or taking a
payload. Native view alternatives retain structural union base/type/index
metadata, including through projected aliases, and compare it at CFG joins.
Native runtime helpers also recheck the active member before payload access.
No native emitter chooses a preferred member or decides exhaustiveness.

Alternative arms lower separately where their payload sources differ. A failed
guard skips the remaining alternatives of that source arm. Partial-pattern
failure releases previously acquired payload views; successful owned arms end
probe loans before moving into distinct owned slots. Mutable bindings update
their selected storage. Typed nested nominal patterns project through recorded
enum layouts into the original storage, so early returns, loop exits, and `try`
need no payload reconstruction. Every enum/union prefix requires its own
physical-source tag proof, including through generic payloads and nested views.
Ancestor loans remain live until their arm-local descendants have ended.
These changes preserve the existing ownership ABI and frame contract. The
explicit-tag layout and drop plans are recorded below; final inline union
storage remains A9 work.

## Batch 1 phase 1: generic union members

Generic bodies execute once for every specialization on both paths, so a
union written as `V | None` reaches the runtime with a symbolic member. The
shared `union_runtime` module gives both backends one rule: a union value
carries the union it was built with and a bare payload; every union
operation aligns that value to the union the instruction names by the active
member's identity (a concrete member of the value's union, or the runtime
type of the payload behind a type-parameter member), retags it in place when
the frame allows mutation, and rejects any active member the instruction's
union cannot admit. Injecting a union-typed `V` payload flattens it into the
destination, so `present[int64 | None](None)` and `absent[int64 | None]()`
are the same value. The interpreter's `NoneTest` and the native
`aura_direct_none_test` helper decide `value is None` on a type-parameter
value at run time. The common validator accepts a union argument that is
the specialization of a generic callee's symbolic union, and the semantic
interface schema is version 10.

## Batch 1 phase 1: union properties

Union equality lives in the shared `PartialEq for Value` and routes through
`union_runtime::union_values_equal`: the active member identities must
match and the payloads must be equal, and a bare member value compares by
the same rule, which is the checker's comparison-only injection with no
runtime union box. Dictionary and set keys of union type reuse that
equality, so a union hashes as its active member. Copy, symbolic copy, and
Transfer classification fold over every member in `sema/properties.rs`;
`.clone()` on a union clones the active payload on both paths. A trait call
on a union receiver lowers to `TraitMember` when every member resolves the
method through one trait: the interpreter dispatches on the payload and
writes a mutable receiver back through the active payload projection, and
the direct backend takes or copies the payload with
`aura_direct_union_active_payload` and writes back through its private
`__union_payload_active` spelling, which never appears in MIR.

## Batch 1 phase 1: union layout and drop plans

`union_layout.rs` owns the explicit-tag plan (ADR-0052 A9): dense canonical
tag ordinals, the smallest unsigned tag width, the widest member alignment
and size, rounding to the aggregate alignment, a size model in which scalars
use their width and every boxed aggregate is one pointer, and per-member
`copy`/`needs_drop` flags. Lowering plans every union a module holds or
operates on into `MirModule.unions`; the common validator recomputes and
compares each plan, refuses duplicates, stale layout versions, foreign
pointer widths, and Copy claims about definitely non-Copy members, and
refuses any union operation whose type has no plan. Both backends ingest
the table and refuse to build or execute an unplanned union. Runtime values
stay tagged boxed values on both paths, so the plan is the shared ABI
contract and identity input rather than a second physical representation;
the semantic interface schema is version 11.

## Batch 1 phase 1: nullable handle FFI results

The one union an extern signature may carry is exactly a declared opaque
handle plus `None`, as a result (ADR-0052 A10, Q10 A). Semantic analysis
admits that shape, through aliases too, and rejects every other extern union
with `AU2010`. The engine's result-only `FfiType::NullableOpaqueHandle`
marshals one C pointer after mutable byte views write back: null yields
`None`, non-null yields the owned handle. Both runtimes construct the result
through the structural union injection helper against the declared union,
so it is the same tagged boxed union as any Aura-built value. The direct
call spec is version 1: a nullable result carries its nominal handle name
and the serialized `Handle | None` union, and decoding refuses any other
combination. `aura_direct_ffi_call` runs inside `task_runtime_boundary`
like every other runtime helper, so an engine failure after the foreign
call returns (a null non-optional handle, a missing symbol, an argument
mismatch) is reported as an `AU4005` diagnostic instead of unwinding
through generated code that carries no unwind tables.

## Batch 1 phase 1: callable contracts

A callable contract is complete (Q17 A, Q19 A): slot names or explicit
positional-only slots, the keyword-only boundary, capabilities, types,
default availability, and the result. `Type` equality stays ABI-only because
both runtimes compare types at run time; `sema/callables.rs` owns the
contract comparison (`callable_slot_admission`, `check_callable_positions`,
`same_callable_contracts`), which the checker applies at every hinted
destination and the shared validator applies at every callable boundary
(`check_callable_contract`) and at every function operand, where the
operand's declared contract is the destination the declaration must satisfy.
A boundary may hide a name, drop default availability, or restrict a named
slot to keyword-only; it may never rename, invent a default, or expose a
keyword-only slot positionally. `MirParam.keyword_only` records the declared
boundary so closure declarations and function operands can be checked, and
the shared binder (`call.rs`) refuses a positional argument that reaches a
keyword-only slot for direct calls, indirect calls, lowering, and both
backends alike. The thin alias adapter `Alias(value)` lowers to the adapted
operand in a temporary typed with the alias contract; no runtime
representation changes. The semantic interface schema is 12.
