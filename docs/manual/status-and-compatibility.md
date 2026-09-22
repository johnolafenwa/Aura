# Status And Compatibility

Aura 0.3.4 is a technical preview. It is suitable for compiler and runtime evaluation, examples, and controlled experiments. It is not a production systems-language release, and it is not a security boundary for untrusted programs.

This page states what the maintained language contract is, which surface it currently covers, and how that surface may change.

## Canonical Contract

The maintained language contract consists of:

1. the normative Language Specification and Manual
2. compiler fixtures and CLI/LSP regression tests as executable conformance evidence
3. the compiler, runtime, CLI, and language server as conforming implementations
4. categorized examples and Learn chapters as teaching material

LSP is the Language Server Protocol. The Manual and the executable suite are expected to agree. A divergence is a project defect, not an alternate language rule.

See [Language Specification](/manual/language-specification) and [Conformance](/manual/conformance). The sections below summarize the current surface by area.

### Reserved Builtin Names

- `select(source, ...)` is an ordinary builtin call. There is no statement form.
- The builtin function name `select` and the builtin enum name `SelectOutcome` are reserved. User declarations with either name are rejected.
- `len` and `str` are also reserved builtin function names. Redefining either is rejected the same way as redefining `print` or `abs`.

### Lengths And Positions

- `str.len()`, `str.byte_len()`, `list.len()`, `dict.len()`, and `set.len()` return `int64`.
- For `str`, `list`, `dict`, and `set`, `len(value)` and `value.len()` have the same static type and value.
- `str.byte_len()` is the separate UTF-8 byte count.
- Range bounds and yields, list indexes, slice endpoints, enumeration positions, and Array coordinates use the same `int64` position domain.

### Expressions, Loops, And Tuples

- Conditional expressions, membership operators, comparison chains, and the `enumerate`/`zip` loop forms are accepted language surface.
- A later `for` loop may reuse the same target names at different element types. Both maintained backends must keep each loop's distinct typed binding identities. See [Statements](/manual/statements#for-iteration) for the loop-form contract.
- Tuples are accepted language surface. Tuple `==` and `!=` compare same-typed values structurally and recursively.
- Reading an existing non-copy tuple for comparison does not consume it. Tuple ordering is rejected. See [Tuples](/manual/tuples).

### Function Values And List Algorithms

- Capture-free named functions are Copy and Transfer values with structural `def(...) -> ...` types.
- Those types are complete contracts. Slots may be named. A `*` boundary makes slots keyword-only in declarations, lambdas, and types. `= ...` promises default availability.
- Bare destinations require identical complete contracts (`AU2015`).
- An explicit thin alias call such as `Unary(function)` may hide names, drop default availability, or restrict a slot to keyword-only. Inference never invents a common contract.
- The eager natural and keyed `list.sort`, `map`, and `filter` algorithms and `control.retry` use this surface. They are current technical-preview APIs.
- `control` resolves as a builtin module namespace. The four List member names are part of the builtin no-shadowing surface.
- Callback capabilities are exact. Code must pass bare or shared element callbacks and cannot rely on adaptation from `mut` or `own`.
- A packed Shared `Callable[...]` or `TaskCallable[...]` value with the matching contract is borrowed at these sites and at `control.retry`.

### Closures And Method Values

- Closures are contextually typed `lambda parameters: expression` expressions.
- Without a capture list, Copy values copy and owned non-Copy values move when the closure is created.
- An exhaustive capture list may have shared, mutable, and owned entries. Shared-loan use is repeatable. Mutable-loan use is repeatable through a mutable closure place. Consuming owned-capture use is single-use.
- A by-value closure is Transfer only when every capture is Transfer. A loan closure is never Transfer.
- A zero-parameter lambda may infer its result without a contextual callable type.
- Capturing closures keep compiler metadata, so they do not cross thin written-`def` parameter, field, collection, or annotated return boundaries.
- To cross those boundaries, a closure is packed explicitly as a `Callable[...]` or `TaskCallable[...]` value. Its call kind may only weaken, and its captures are owned. A body may mutate an owned capture, which makes the closure Mutable.
- `receiver.method` outside call position is a bound method. It is a compiler-synthesized closure over the receiver, and its call kind follows the receiver capability.
- `Class.method` on a non-generic class is a function value.
- A stored callable's contract may return a view of one explicit parameter: `def(...) -> view [mut] T from name`. A call through it binds a `view` whose footprint is every fixed path of the origin, with the result type.
- Captured-self origins and loan environments cannot be stored. Task callables cannot return views.

### Views

- Place-based loans and views are implemented. Local shared and mutable views cover roots, fixed class fields, tuple positions, list elements, and dictionary entries.
- A view's lifetime ends after its conservative final use. Reborrows keep source identity. Mutable views write through immediately.
- One declared receiver or parameter may be the origin of `-> view [mut] T from source`.
- MIR execution, direct native builds, analysis/LSP, and editor tooling share the semantic interface for this surface. MIR is Aura's mid-level intermediate representation.
- Views and loan closures are task-local and non-Transfer.
- [Current Limits](/manual/current-limits) lists the places that cannot be loaned.

### Foreign Functions

- Explicitly authorized packages may use FFI v0, the version 0 foreign function interface.
- Bodyless `extern "C"` functions call process-global symbols synchronously through fixed-width scalars, pointer-length str and byte views, or non-null opaque handles.
- FFI-enabled dependencies must be visible in the root manifest's exact `[ffi] dependencies` report.
- Externs are direct-call-only.
- An extern result may be exactly `Handle | None`, marshalled as one nullable C pointer.
- Callbacks, raw pointers, variadics, returned views, other nullable shapes, and explicit library loading are not available.
- FFI is an unsafe native boundary. It is not a memory safety promise for a false declaration or a misbehaving C implementation.

### Comprehensions

- List, set, and dictionary comprehensions are eager and owned.
- Clauses inherit statement bare-loop iteration. That includes shared List and set traversal, Range copy values, compiler-known `enumerate`/`zip`, and Queue's receive-owned item carve-out.
- Nested clauses are outer-major. Filters run left to right. Dictionary keys run before values. Target names never leak.
- Result insertion follows the ordinary Copy, move, explicit-clone, and closure capture rules.
- Generator expressions are not available. Use an eager comprehension or an explicit loop.

### Slices

- Owned list and str slicing uses four one-colon forms. Endpoints may be omitted, and the result is a half-open range.
- Written endpoints use the `int64` position domain. Negatives normalize once. An invalid or reversed range traps with `AU4003`. Endpoints are not clamped.
- String positions count Unicode scalar values and require an O(n) scan.
- Every result owns independent storage. List elements copy or clone under the clone-safety and task-repeatability rules. A str slice produces a fresh valid UTF-8 value.
- String integer indexing, steps, slice assignment, and indexed slice views are not available.
- The `view` forms do not reinterpret owned slice syntax.

### Numeric Arrays

- `Array[T]` is a global contiguous array type. The four dtypes are `int32`, `int64`, `float32`, and `float64`.
- Every value owns a rank-at-least-one row-major CPU buffer.
- The surface includes three constructors, multidimensional scalar indexing, first-axis owned slices, mutation, mapping, reductions, exact-shape and scalar kernels, and explicit wrapping and saturating integer arithmetic.
- There is no array-shape broadcasting, mixed promotion, views, shape transformations, equality, autograd, or accelerator placement.
- `mean()` returns `float64` for every dtype. Integer Array `/` is rejected, following the integer-division rule.

### Integer And Math Operations

- Integer literals support decimal separators and hexadecimal, binary, and octal bases.
- Every integer width supports fixed-width bitwise operators, checked shifts, and explicit wrapping and saturating shift modes.
- `**` provides checked same-type integer power and same-type floating power.
- `round` implements ties-to-even floating conversion to `int64` and exact integer identity.
- `divmod` returns the paired floor quotient and divisor-signed remainder.

## Stability Policy

- Accepted architecture decision records (ADRs) define the current reference baseline.
- Outside explicitly recorded decisions, syntax expansion is frozen for each technical-preview release.
- Work prioritizes correctness, native-runtime safety, editor responsiveness, and a coherent control-plane surface.
- APIs may change while Aura remains a technical preview.

The Manual is reference-frozen. Every semantic change, including any extension, requires an ADR. It must update the normative reference, compiler fixtures, maintained examples, and tutorials in the same commit. A change that cannot keep those surfaces synchronized does not enter the maintained language.

Compiler coverage is held at the current non-regression floor, not pushed to 100%. New behavior still requires focused tests. The freeze only ends marginal coverage work that does not reduce product risk.

Seeded randomness has an extra observable-data promise. The algorithm, seed mapping, integer and floating mappings, and shuffle order documented in [Randomness Module](/manual/randomness) stay stable throughout Aura 0.3.x. A later release may change them only with an explicit decision and new conformance vectors. OS-secure outputs are intentionally not stable.

## Maintained Concurrency Surface

Aura 0.3 uses structured concurrency:

- `TaskGroup()` owns child tasks inside `with`.
- `TaskGroup.start(...)` returns a `Task[T]`.
- `TaskGroup.start_soon(...)` starts a child whose result is not retained.
- Default task stacks are guarded and 768 KiB. `TaskGroup.start_with_stack(...)` and `start_soon_with_stack(...)` override the size, from the 256 KiB minimum for measured shallow tasks through 64 MiB.
- `Queue[T]` provides bounded or unbounded task-aware communication.
- `yield_now()` provides an explicit cooperative scheduling point.
- `select(...)` provides a typed heterogeneous Queue/Task/deadline wait.
- `wait_any(...)` and `wait_all(...)` coordinate task completion.

There is no `Channel`, statement-form `select`, bare `spawn`, or detached task. `select(...)` is an ordinary builtin call and adds no branch syntax.

### Workers

Task bodies run on pinned cooperative scheduler workers on both maintained backends.

- The default worker count is the available parallelism reported by the host. The provisional `AURA_WORKERS=<positive integer>` override selects an explicit count.
- A child receives a stable worker assignment when it is spawned. Its coroutine stack never migrates, and work is not stolen.
- `yield_now()` yields only to runnable work on the same worker.
- Compiler-inserted checks on every loop backedge keep a tight loop from starving ready timers, Queue operations, and sockets on the same worker indefinitely. Ordinary loop tails and `continue` take part. `break` and `return` bypass the backedge.
- The checks do not inspect cancellation. One long loop body can still delay same-worker siblings.
- Ordinary tasks request a guarded 768 KiB coroutine stack. Explicit requests may go up to 64 MiB.
- Waits use persistent descriptor registrations, heap-managed deadlines, and direct Queue, task-completion, and blocking-pool notifications. An idle worker blocks until work, an event, or a deadline, with no periodic tick.

### Blocking-I/O Pool

A separate process-wide pool runs blocking I/O. It is lazily initialized.

- The first runtime preflight reads the pool settings once, without starting worker threads. The configuration then stays fixed for the process lifetime.
- The first blocking submission creates the complete worker set. Production reuses it until process exit and exposes no Aura shutdown or join surface.
- `AURA_BLOCKING_WORKERS` selects an exact positive worker count. Without it, the runtime derives a default from host parallelism with fallback `4` and clamps it to `2..=8`.
- `AURA_BLOCKING_QUEUE_CAPACITY` optionally bounds accepted pending jobs only. When it is omitted, the pending queue is unbounded.
- Full-queue admission is FIFO and scheduler-aware.
- MIR, direct, and standalone execution reject invalid values with `AU4006` before user code runs.
- Cancellation or timeout before queue insertion prevents submission. Accepted work runs once, and an abandoned result is discarded.
- The bound cannot interrupt host calls or guarantee unrelated blocking-I/O progress while all workers are occupied.

### Transfer Across Tasks

- The compiler derives structural Transfer checks for task captures, task results, and Queue payloads.
- Task handles are Copy only under conditions, and non-repeatable results are statically single-consumer.
- Queue and Task handles are the maintained cross-worker channels. All other boundary values stay owned and share-nothing through `Transfer`.
- Cancellation and diagnostic context stay per task.
- Scheduling, completion, and program-output order are unspecified.
- Task execution is multicore. Preemption, work stealing, worker introspection, and detached tasks are not available. Parallel speedup depends on the program.

See [Execution Model](/manual/execution-model) and [Current Limits](/manual/current-limits).

### Runtime Frames And Schemas

- Both maintained backends produce complete typed runtime frames. Diagnostics carry innermost-first Aura call frames and youngest-first task ancestry.
- Each public schema-version-1 frame span has its own required source `path`. The analysis/LSP editor shape permits an optional `file_path` for source-only analysis.
- The public diagnostic schema stays at version `1`, because the always-present arrays are an additive extension.
- Compiler-service and editor transport uses semantic schema version `16`. This version includes structural function values, import aliases, and the expanded numeric expression surface. It forwards the same diagnostic records.

## Platform And Distribution Support

Release archives target glibc Linux x86-64 and macOS x86-64/Apple silicon. Each archive includes the native runtime and linker manifest used by `aura build`. Cargo and the Aura source checkout are not runtime dependencies of an installed archive. A host C compiler is still required.

See the repository `SUPPORTED_PLATFORMS.md` for the exact matrix and pinned toolchain.

## Design Record

- [ADR-0002: Integer division and modulo](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0002-integer-division-and-modulo.md)
- [ADR-0026: Minimal tuples](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0026-minimal-tuples.md)
- [ADR-0027: Conditional expressions](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0027-conditional-expressions.md)
- [ADR-0028: Membership operators and comparison chains](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0028-membership-and-comparison-chains.md)
- [ADR-0029: `enumerate` and `zip` loop forms](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0029-enumerate-and-zip-loop-forms.md)
- [ADR-0030: `len` and `str` builtins](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0030-len-and-str-builtins.md)
- [ADR-0032: Guarded lightweight-task stacks](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0032-guarded-lightweight-task-stacks.md)
- [ADR-0033: Structural Transfer and task-result consumption](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md)
- [ADR-0034: Typed heterogeneous `select`](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0034-typed-heterogeneous-select.md)
- [ADR-0035: Configurable blocking-I/O pool](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0035-configurable-blocking-io-pool.md)
- [ADR-0036: Native structured runtime frames](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0036-native-structured-runtime-frames.md)
- [ADR-0037: Expression closures and value capture](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0037-expression-closures-and-value-capture.md)
- [ADR-0038: Place-based loans and views](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0038-place-based-loans-and-views.md)
- [ADR-0039: Comprehensions](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0039-comprehensions.md)
- [ADR-0040: Owned list and str slices](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0040-owned-vec-and-string-slices.md)
- [ADR-0041: Contiguous numeric arrays and explicit integer arithmetic modes](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0041-contiguous-numeric-arrays.md)
- [ADR-0047: Integer literal bases, bitwise operators, and shifts](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0047-integer-literals-bitwise-and-shifts.md)
- [ADR-0048: Power, rounding, divmod, and the math module](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0048-power-round-divmod-and-math.md)
