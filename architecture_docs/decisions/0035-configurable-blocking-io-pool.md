# ADR-0035: Configurable blocking-I/O pool

- Status: Accepted
- Date: 2026-07-27
- Accepted: 2026-07-29
- Roadmap decision: Batch 4, Phase 5.9
- Related: ADR-0019, ADR-0032, ADR-0033, and ADR-0034

## Context

Aurora moves host operations that may block, including filesystem work and
name resolution, off lightweight-task stacks and onto one process-wide
blocking-I/O pool. The current pool chooses the host's available parallelism,
falls back to four workers, clamps the result to two through eight, and keeps
an unbounded FIFO job queue.

Timing out or cancelling an Aurora wait cannot interrupt a host call that has
already begun. The Aurora task resumes and safely abandons the eventual
result, but the host call continues to occupy a blocking worker. A resolver
outage can therefore occupy every worker while further abandoned resolution
jobs accumulate ahead of unrelated filesystem work. The unbounded queue
offers neither an operator control for the worker count nor an admission point
at which a task can still honour its deadline or cancellation.

This decision configures the existing generic blocking-I/O pool. It does not
merge that pool with the bounded protocol-step service or the JSON codec
service, and it does not claim that Aurora can forcibly cancel an accepted
host operation.

## Decision

### Process configuration

The generic blocking-I/O pool has two process environment settings:

- `AURORA_BLOCKING_WORKERS=<positive integer>` selects an explicit worker
  count. When it is absent, the runtime uses
  `available_parallelism`, falls back to `4` if the host cannot report it, and
  clamps that derived default to `2..=8`. An explicit value is not clamped:
  the requested positive count is used exactly or pool creation fails.
- `AURORA_BLOCKING_QUEUE_CAPACITY=<positive integer>` bounds the number of
  accepted jobs waiting in the pool queue. When it is absent, the queue is
  unbounded for compatibility with the pre-Phase-5.9 runtime.

Zero, an empty value, a signed or otherwise non-decimal value, and a value
that does not fit the runtime's supported integer range are invalid. A present
invalid setting is a fatal `AU4006` runtime-configuration diagnostic that
names the setting, renders its value, and says that a positive integer is
required.

Configuration is read and validated once per process during the first runtime
preflight and remains immutable for that process lifetime. Validation happens
before any Aurora user code executes. The same preflight is mandatory for
`aura run --backend mir`, `aura run --backend direct`, and a launched
standalone native binary. Invalid configuration therefore fails before
`main`, module initialization, a top-level test body, or another user-visible
side effect on every execution path. Backend selection and direct-runtime
cache state do not change the diagnostic code or message.

Preflight records an immutable configuration; it does not start the pool.
The pool remains lazy and creates no worker threads until the first blocking
job needs submission. Tests use explicit injected configurations rather than
mutating process environment after preflight.

### Fallible lazy initialization

The first submission initializes the complete configured worker set before it
accepts a job. Workers have stable diagnostic names, and pool startup is
all-or-nothing. If any configured worker cannot be created, initialization
shuts down the workers already created for that attempt, accepts no job, and
returns `AU4006`. The diagnostic identifies blocking-I/O worker creation as
the failed runtime configuration and preserves the host error as detail.

Initialization failure is cached for the process lifetime. Later calls do
not repeatedly create partial worker sets or silently run with fewer workers.
Panics inside an individual job remain contained as operation failures and do
not silently reduce the configured pool size.

### Capacity and FIFO admission

Queue capacity counts accepted jobs that are pending in the pool queue. Jobs
currently executing on workers do not consume queue capacity, and tasks
waiting for admission have not yet become jobs and do not consume it either.
For example, with two workers and capacity three, up to two jobs may be
running while three more are accepted and pending.

Workers remove accepted jobs from the pending queue in FIFO order. Removing a
job opens one admission slot. When a bounded queue has no slot, submitters
wait in their own FIFO admission queue. The oldest still-live waiter receives
the next slot; a cancelled or timed-out waiter is removed without allowing a
younger waiter to pass an older live one. Job completion order remains
unspecified because host operations run concurrently.

Admission from an Aurora lightweight task is scheduler-aware. A full queue
parks the task through the persistent reactor/wakeup path and leaves its
pinned worker free to run other tasks. A slot release, deadline, or
cancellation directly wakes the pinned task; admission does not poll, block
an Aurora worker thread, create a helper task, or add a periodic scheduler
tick. Host-side runtime callers that are not executing as lightweight tasks
use the same FIFO state and may block their calling host thread while waiting
for a slot.

The admission protocol is check-register-recheck:

1. check the operation's absolute deadline and cancellation state
2. insert immediately if a pending-queue slot is available
3. otherwise register one FIFO admission waiter
4. recheck the deadline, cancellation, and queue state before parking
5. on wake, either remove the waiter and return the timeout/cancellation, or
   let the oldest live waiter atomically consume the next slot

Waiter removal and slot handoff are idempotent. A release racing a
deadline/cancellation cannot leak a slot, admit the job twice, leave a stale
wakeup registration, or strand the next FIFO waiter.

### Submission and abandonment boundary

Insertion into the pending job queue is the acceptance linearization point.
Before that point, expiry or cancellation ends admission promptly: the
operation is never submitted, never runs later, and produces the operation's
ordinary timeout or cancellation result.

After acceptance, the job cannot be retracted. A timeout or cancellation may
still end the Aurora task's wait promptly, but the pending or running host job
will execute exactly once. Its eventual result is discarded if no caller is
still waiting. Late success, host error, or panic may release retained runtime
state and signal a closed completion, but may not resume the abandoned task,
publish a value into user code, or overwrite a newer operation.

This boundary is deliberately conservative. Rust's blocking host APIs do not
provide a general safe interruption contract, and removing an accepted job
would make whether an externally visible filesystem or resolver operation ran
depend on a queue race. A future cancellable-host-operation facility requires
a separate decision.

### Resolver-outage saturation contract

The Phase-5.9 stress test uses injected, gated resolver work rather than a
real DNS outage. Real network state is too nondeterministic for a semantic
gate. With an explicit small worker count and bounded pending capacity, the
test must prove this matrix:

| Saturation state | Required completion |
| --- | --- |
| Every worker holds a gated resolver job; the pending queue still has room | Later resolver jobs are accepted in FIFO order. A caller deadline/cancellation may finish its wait, but each accepted job runs once after a gate opens and its late result is discarded. |
| Every worker is occupied and the pending queue is full | A further deadline-bound resolver request ends before submission with timeout; its injected operation counter remains zero. |
| Every worker is occupied and the pending queue is full | A further cancellable resolver request ends before submission with cancellation; its injected operation counter remains zero. |
| Two or more live submissions await one full queue | Opened slots admit them in registration order; removing a timed-out/cancelled waiter preserves the order of the remaining live waiters. |
| Accepted resolver jobs are abandoned, then the outage gates open | All accepted jobs drain exactly once, every slot is recovered, and an unrelated blocking filesystem probe is admitted and completes without restarting the pool. |
| Completion, slot release, cancellation, and deadline race | Exactly one admission outcome is observed, no job executes twice, no capacity is lost, no stale waiter is retained, and later unrelated work completes. |

The matrix runs repeatedly against the shared runtime primitive and through
forced MIR and direct product paths. The product fixture also proves that a
hot Aurora task continues to make progress while another task is parked for
blocking-pool admission. An unbounded-queue control preserves the existing
accept-without-admission-wait behavior.

### Backend and diagnostic contract

MIR and direct-native adapters call the same blocking-pool submission
primitive and inherit one configuration, admission, abandonment, and
completion protocol. Standalone native binaries include that shared runtime
contract. No backend may fall back to synchronous execution on an Aurora
worker when pool initialization or admission fails.

The diagnostic contract is:

- invalid `AURORA_BLOCKING_WORKERS` uses fatal `AU4006` before user code
- invalid `AURORA_BLOCKING_QUEUE_CAPACITY` uses fatal `AU4006` before user
  code
- failure to create the explicitly or implicitly configured blocking workers
  uses `AU4006` when the lazy pool first initializes
- operation deadlines, cancellation, and host I/O failures retain their
  existing operation-level result/diagnostic contracts; capacity saturation
  does not invent a new overload error

The settings are operational controls, not Aurora language syntax or a stable
library API. They do not change type checking, ownership, task pinning, or
the `Transfer` rules.

## Consequences

Operators can increase blocking concurrency for resolver-heavy workloads or
bound the accepted backlog. The default worker behavior is unchanged, and an
absent queue-capacity setting retains the unbounded compatibility mode.
Choosing an excessive explicit worker count is no longer silently corrected;
the host either creates exactly that set or Aurora reports the failure.

A bounded queue limits pending accepted work, not work already executing.
It therefore cannot cure an indefinitely stuck host call or guarantee that
unrelated work starts before occupied workers return. It does give
deadline/cancellation a scheduler-friendly pre-submission boundary and keeps
an outage from building an unlimited accepted backlog.

FIFO is defined for pending jobs and admission waiters, not for completion.
An admitted filesystem job can still wait behind earlier resolver jobs, and
several running jobs may complete in any order. Applications needing stronger
isolation require separate pools or evented resolver/filesystem facilities,
which are outside this decision.

## Completion-test matrix

| Contract | Required evidence |
| --- | --- |
| Worker configuration | Absent setting covers host parallelism, fallback `4`, and derived `2..=8` clamp; explicit positive values below, within, and above that range are used exactly without a clamp. |
| Queue configuration | Absent capacity is unbounded; explicit positive capacities, including `1`, bound pending jobs only while running jobs and admission waiters are accounted separately. |
| Fatal validation | Empty, zero, signed, non-decimal, non-Unicode, and overflowing values for each setting produce stable `AU4006` diagnostics before any user side effect under forced MIR, forced direct, and standalone launch. |
| Lazy lifecycle | Valid preflight starts no thread; first submission creates exactly the configured set once; later submissions reuse it for the process lifetime. Injected test pools prove that test-only shutdown rejects parked admissions, drains accepted work, and joins the worker set; production workers otherwise live until process exit. |
| Creation failure | Deterministic failure at every worker index proves all-or-nothing startup, cleanup of earlier workers, no accepted job, cached failure, and `AU4006` with host detail. |
| Capacity accounting | Controlled running, pending, and waiting jobs prove that only pending jobs consume capacity and every dequeue/failed admission restores exactly one slot. |
| FIFO behavior | Pending execution order and waiter admission order are pinned; cancellation/deadline removal preserves the order of all remaining live waiters. |
| Scheduler integration | A lightweight task parks on full-queue admission without blocking its pinned worker; direct slot, deadline, and cancellation wakeups require no polling, helper task, migration, or periodic tick. |
| Admission races | Check-register, register-recheck, slot-versus-deadline, and slot-versus-cancellation injections prove one outcome, no missed wake, no double submission, no leaked capacity, and idempotent cleanup. |
| Accepted-job abandonment | Timeout and cancellation before acceptance prevent execution; after acceptance they resume the caller promptly while the job executes once and its late success/error/panic is discarded safely. |
| Resolver saturation | The complete injected resolver-outage matrix above drains accepted work and allows an unrelated filesystem probe to complete after saturation on repeated runs. |
| Backend parity | Shared-runtime, forced-MIR, forced-direct, and standalone cases agree on configuration failures, FIFO admission, timeout/cancellation, late-result disposal, and observable completion. |
| Compatibility and reference | Existing blocking I/O succeeds with both settings absent; maintained runtime/reference pages document defaults, capacity accounting, abandonment, and the fact that bounded admission cannot interrupt an accepted host call. |

The Phase 5.9 implementation passed the focused unit/race/stress,
forced-backend, standalone, reference, full-CI, benchmark, and frozen-coverage
gates. Batch 5 then serialized only the two mutually contending saturated-pool
watchdogs and proved them repeatedly under default parallel test execution,
completing the matrix without changing product behavior.
