# Status And Compatibility

Aurora 0.1 is an advanced technical preview. It is suitable for compiler and runtime evaluation, examples, and controlled experiments; it is not yet a production systems-language release or a security boundary for untrusted programs.

## Canonical Contract

The maintained language contract consists of:

1. the normative Language Specification and Manual
2. compiler fixtures and CLI/LSP regression tests as executable conformance evidence
3. the compiler, runtime, CLI, and language server as conforming implementations
4. categorized examples and Learn chapters as teaching material

The Manual and executable suite are expected to agree. A divergence is a
project defect, not an alternate language rule. The historical proposal is
design history. Features mentioned only there—including `Channel`,
statement-form `select`, detached spawn, attributes, and registry
publishing—are not part of Aurora 0.1. Provisional ADR-0034 instead adds the
ordinary builtin call `select(source, ...)`; it adds no statement syntax.
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

See [Language Specification](/manual/language-specification) and [Conformance](/manual/conformance).

## Stability Policy

The ratified correctness-recovery and Phase 1.5 semantic re-defaults are the final pre-reference language changes for 0.1. Outside explicitly recorded ADR decisions, syntax expansion is frozen for the hardening cycle. Work in this cycle prioritizes correctness, native-runtime safety, editor responsiveness, and an honest control-plane surface. APIs may still change while 0.1 remains untagged.

The post-Phase-1.5 Manual is reference-frozen. Every later semantic change,
including a compatible extension, requires an ADR and must update the normative
reference, compiler fixtures, maintained examples, and tutorials in the same
commit. A change that cannot keep those surfaces synchronized does not enter
the maintained language.

Compiler coverage is held at the current non-regression floor rather than being pushed to 100%. New behavior still requires focused tests; the freeze only ends marginal coverage work that does not reduce product risk.

Seeded randomness has an additional observable-data promise: the algorithm,
seed mapping, integer and floating mappings, and shuffle order documented in
[Randomness Module](/manual/randomness) remain stable throughout Aurora 0.1.x.
A later compatibility series may change them only with an explicit decision
and new conformance vectors. OS-secure outputs are intentionally not stable.

## Maintained Concurrency Surface

Aurora 0.1 uses structured concurrency:

- `TaskGroup()` owns child tasks inside `with`
- `TaskGroup.start(...)` returns a `Task[T]`
- `TaskGroup.start_soon(...)` starts a child whose result is not retained
- Provisional ADR-0032 adds guarded 512 KiB default task stacks plus
  `TaskGroup.start_with_stack(...)` and `start_soon_with_stack(...)` overrides
  from the measured-shallow-task 256 KiB minimum through 64 MiB
- `Queue[T]` provides bounded or unbounded task-aware communication
- `yield_now()` provides an explicit cooperative scheduling point
- `select(...)` provides a typed heterogeneous Queue/Task/deadline wait under
  Provisional ADR-0034
- `wait_any(...)` and `wait_all(...)` coordinate task completion

There is no `Channel`, statement-form `select`, bare `spawn`, or detached
task. `select(...)` is an ordinary builtin call; it does not add branch syntax.

Task bodies execute on pinned cooperative scheduler workers on both maintained
backends. The default worker count is the available parallelism reported by
the host; the
provisional `AURORA_WORKERS=<positive integer>` override selects an explicit
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

Provisional ADR-0033 implements compiler-derived structural Transfer checks for
task captures, task results, and Queue payloads, plus conditional task-handle
Copy and statically single-consumer non-repeatable results. Queue and Task
handles are the maintained cross-worker channels; all other boundary values
remain owned and share-nothing through `Transfer`. Cancellation and diagnostic
context stay per task. Scheduling, completion, and program-output order are
unspecified. This is a multicore guarantee for task execution, not a guarantee
of preemption, work stealing, worker introspection, detached tasks, or parallel
speedup for every program. See [Execution Model](/manual/execution-model) and
[Current Limits](/manual/current-limits).

## Platform And Distribution Support

Release archives target glibc Linux x86-64 and macOS x86-64/Apple silicon. Each archive includes the native runtime and linker manifest used by `aura build`; Cargo and the Aurora source checkout are not runtime dependencies of an installed archive. A host C compiler is still required.

See the repository `SUPPORTED_PLATFORMS.md` for the exact matrix and pinned toolchain.
