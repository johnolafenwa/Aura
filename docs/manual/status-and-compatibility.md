# Status And Compatibility

Aura 0.2 is an advanced technical preview. It is suitable for compiler and runtime evaluation, examples, and controlled experiments; it is not yet a production systems-language release or a security boundary for untrusted programs.

## Canonical Contract

The maintained language contract consists of:

1. the normative Language Specification and Manual
2. compiler fixtures and CLI/LSP regression tests as executable conformance evidence
3. the compiler, runtime, CLI, and language server as conforming implementations
4. categorized examples and Learn chapters as teaching material

The Manual and executable suite are expected to agree. A divergence is a
project defect, not an alternate language rule. Aura provides the ordinary
builtin call `select(source, ...)` under Accepted ADR-0034 and no statement
form.
That addition reserves both the builtin function name `select` and builtin
enum name `SelectOutcome`; existing user declarations with either name must
be renamed.
Adding `len` and `str` to the maintained builtin functions also reserves both
names: a program that previously declared its own `def len(...)` or
`def str(...)` is now rejected, the same way redefining `print` or `abs` is.
ADR-0030 is Accepted with the B3.0-d length-unification amendment:
`String.len()`, `String.byte_len()`, `Vec.len()`, `Map.len()`, and `Set.len()`
now return `int64`. For `String`, `Vec`, `Map`, and `Set`, `len(value)` and
`value.len()` have the same static type and value; `String.byte_len()` is the
separate UTF-8 byte count. Code that previously annotated a member length as
`int32`, or passed one directly to a still-`int32` range or Vec index boundary,
must update the annotation or use an explicit checked `as int32` cast.

Conditional expressions, membership operators, comparison chains, and the
`enumerate`/`zip` loop forms are accepted language surface under ADR-0027,
ADR-0028, and ADR-0029. A later `for` loop may reuse the same target names at
different element types; both maintained backends must preserve each loop's
distinct typed binding identities. Tuples are accepted language surface under
ADR-0026. Tuple `==` and `!=` compare same-typed values structurally and
recursively. Reading an existing non-copy tuple for comparison does not
consume it; tuple ordering remains rejected. See
[Tuples](/manual/tuples) and
[Statements](/manual/statements#for-iteration) for the loop-form contract.

Phase 6.1 capture-free function values make named functions Copy and Transfer
values with structural `def(...) -> ...` types. Phase 6.2 uses that surface for
the maintained eager `Vec.sort`, `sort_by`, `map`, and `filter` algorithms and
for `control.retry`. These are compatible API additions to the technical
preview, but `control` now resolves as a builtin module namespace and the four
Vec member names are part of the builtin no-shadowing surface. Callback
capabilities are exact: code must pass bare/shared element callbacks rather
than relying on adaptation from `mut` or `own`.

Contextually typed `lambda parameters: expression` closures follow Accepted
ADR-0037. Captures are by value: Copy values are copied,
owned non-Copy values move at creation, read-only capture use is repeatable,
and consuming capture use makes the closure single-use. A closure is Transfer
only when every capture is Transfer. Shared or mutable capability capture and
mutable captured state are unavailable. Zero-parameter lambdas may infer their result
without a contextual callable type. Capturing closures retain compiler
metadata and therefore do not cross arbitrary written-`def` parameter, field,
collection, or annotated return boundaries.

Phase 6.4 adds explicitly authorized FFI v0 packages. Bodyless
`extern "C"` functions call process-global symbols synchronously through
fixed-width scalars, pointer-length String/byte views, or non-null opaque
handles. FFI-enabled dependencies must be visible in the root manifest's exact
`[ffi] dependencies` report. Externs are direct-call-only; callbacks, raw
pointers, variadics, returned views, nullable handles, and explicit library
loading remain unavailable. This is an unsafe native boundary, not a memory
safety promise for a false declaration or misbehaving C implementation.

Phase 7.1 adds eager owned list, set, and map comprehensions under Accepted
ADR-0039. Clauses inherit statement bare-loop iteration, including shared
Vec/Set traversal, Range copy values, compiler-known `enumerate`/`zip`, and
Queue's receive-owned item carve-out. Nested clauses are outer-major, filters
run left to right, map keys run before values, and target names never leak.
Result insertion follows ordinary Copy, move, explicit-clone, and ADR-0037
capture rules. Generator expressions remain unavailable and
receive an `AU2005` migration to an eager comprehension or explicit loop.

Phase 7.2 adds owned Vec and String slicing under Accepted ADR-0040. The four
one-colon forms accept omitted endpoints and select a half-open range. Written
endpoints are exact `int32`; negatives normalize once, and an invalid or
reversed range traps with `AU4003` instead of adopting Python's clamping
behavior. String positions count Unicode scalar values and require an O(n)
scan. Every result owns independent storage: Vec elements copy or clone under
clone-safety and task-repeatability rules, while String produces a fresh valid
UTF-8 value. String integer indexing, steps, slice assignment, and views remain
unavailable; this feature does not implement ADR-0038.

Phase 7.3 adds global contiguous `Array[T]` values under Accepted ADR-0041.
The four dtypes are `int32`, `int64`, `float32`, and `float64`; every value
owns a rank-at-least-one row-major CPU buffer. The accepted surface includes
three constructors, multidimensional scalar indexing, first-axis owned
slices, mutation, mapping, reductions, exact-shape/scalar kernels, and
explicit wrapping/saturating integer arithmetic. It adds no array-shape broadcasting,
mixed promotion, views, shape transformations, equality, autograd, or
accelerator placement. `mean()` returns `float64` for every dtype; integer
Array `/` remains rejected under ADR-0002.

See [Language Specification](/manual/language-specification) and [Conformance](/manual/conformance).

## Stability Policy

The ratified correctness-recovery and Phase 1.5 semantic re-defaults established the reference baseline that the 0.2 capability migration and later Accepted ADRs extend. Outside explicitly recorded ADR decisions, syntax expansion is frozen for the technical-preview release. Work in this cycle prioritizes correctness, native-runtime safety, editor responsiveness, and an honest control-plane surface. APIs may still change while 0.2 remains a technical preview.

The post-Phase-1.5 Manual is reference-frozen. Every later semantic change,
including a compatible extension, requires an ADR and must update the normative
reference, compiler fixtures, maintained examples, and tutorials in the same
commit. A change that cannot keep those surfaces synchronized does not enter
the maintained language.

Compiler coverage is held at the current non-regression floor rather than being pushed to 100%. New behavior still requires focused tests; the freeze only ends marginal coverage work that does not reduce product risk.

Seeded randomness has an additional observable-data promise: the algorithm,
seed mapping, integer and floating mappings, and shuffle order documented in
[Randomness Module](/manual/randomness) remain stable throughout Aura 0.2.x.
A later compatibility series may change them only with an explicit decision
and new conformance vectors. OS-secure outputs are intentionally not stable.

## Maintained Concurrency Surface

Aura 0.2 uses structured concurrency:

- `TaskGroup()` owns child tasks inside `with`
- `TaskGroup.start(...)` returns a `Task[T]`
- `TaskGroup.start_soon(...)` starts a child whose result is not retained
- Accepted ADR-0032 adds guarded 512 KiB default task stacks plus
  `TaskGroup.start_with_stack(...)` and `start_soon_with_stack(...)` overrides
  from the measured-shallow-task 256 KiB minimum through 64 MiB
- `Queue[T]` provides bounded or unbounded task-aware communication
- `yield_now()` provides an explicit cooperative scheduling point
- `select(...)` provides a typed heterogeneous Queue/Task/deadline wait under
  Accepted ADR-0034
- `wait_any(...)` and `wait_all(...)` coordinate task completion

There is no `Channel`, statement-form `select`, bare `spawn`, or detached
task. `select(...)` is an ordinary builtin call; it does not add branch syntax.

Task bodies execute on pinned cooperative scheduler workers on both maintained
backends. The default worker count is the available parallelism reported by
the host; the
provisional `AURA_WORKERS=<positive integer>` override selects an explicit
count. A child receives a stable worker assignment when it is spawned. Its
coroutine stack never migrates, work is not stolen, and `yield_now()` yields
only to runnable work on that worker.

Compiler-inserted checks on every loop backedge prevent a tight loop from
starving ready timers, Queue operations, and sockets assigned to the same
worker indefinitely. Ordinary loop tails and `continue` participate; `break`
and `return` bypass the backedge. The checks do not inspect cancellation, and
one long loop body can still delay same-worker siblings. Ordinary tasks request
a guarded 512 KiB coroutine stack; explicit requests may range through 64 MiB.
Waits
use persistent descriptor registrations, heap-managed deadlines, and direct
Queue, task-completion, and blocking-pool notifications; an idle worker blocks
until work, an event, or a deadline without a periodic tick.

Under Accepted ADR-0035, the separate process-wide blocking-I/O pool is
lazily initialized. The first runtime preflight reads its settings once and
keeps that configuration immutable for the process lifetime without starting
worker threads. First blocking submission creates the complete worker set;
production reuses it until process exit and exposes no Aura shutdown/join
surface.
`AURA_BLOCKING_WORKERS` selects an exact positive worker count; without it,
the runtime derives and clamps a `2..=8` default from host parallelism with
fallback `4`. `AURA_BLOCKING_QUEUE_CAPACITY` optionally bounds accepted
pending jobs only, while omission preserves the unbounded compatibility mode.
Full-queue admission is FIFO and scheduler-aware. MIR, direct, and standalone
execution reject invalid values with `AU4006` before user code. Cancellation
or timeout before queue insertion prevents submission; accepted work runs once
and has any abandoned result discarded. The bound cannot interrupt host calls
or guarantee unrelated blocking-I/O progress while all workers remain
occupied.

Accepted ADR-0033 implements compiler-derived structural Transfer checks for
task captures, task results, and Queue payloads, plus conditional task-handle
Copy and statically single-consumer non-repeatable results. Queue and Task
handles are the maintained cross-worker channels; all other boundary values
remain owned and share-nothing through `Transfer`. Cancellation and diagnostic
context stay per task. Scheduling, completion, and program-output order are
unspecified. Task execution is multicore; preemption, work stealing, worker
introspection, and detached tasks are unavailable, while parallel speedup
depends on the program. See [Execution Model](/manual/execution-model) and
[Current Limits](/manual/current-limits).

Accepted ADR-0036 defines complete typed runtime frames on both maintained
backends. Diagnostics carry innermost-first Aura call frames and
youngest-first task ancestry. Each public schema-version-1 frame span has its
own required source `path`; the analysis/LSP editor shape permits an optional
`file_path` for source-only analysis. The public diagnostic schema remains
version `1` because the always-present arrays are an additive extension;
compiler-service/editor transport uses semantic schema version `3`, which adds
structural function types and function-value operands, and
forwards the same records.

## Platform And Distribution Support

Release archives target glibc Linux x86-64 and macOS x86-64/Apple silicon. Each archive includes the native runtime and linker manifest used by `aura build`; Cargo and the Aura source checkout are not runtime dependencies of an installed archive. A host C compiler is still required.

See the repository `SUPPORTED_PLATFORMS.md` for the exact matrix and pinned toolchain.
