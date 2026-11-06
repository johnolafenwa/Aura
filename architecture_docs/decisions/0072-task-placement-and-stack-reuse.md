# ADR-0072: Task placement, stealing, and stack reuse

- Status: Accepted
- Date: 2026-11-03
- Builds on: ADR-0032
- Design: [Batch 8 design checkpoint](../26-batch-8-design-checkpoint.md),
  ratified 2026-11-03 (decisions A1, B1, C1, D1, E1, F1)

## Context

Before Batch 8 the scheduler placed every child on the next worker in
round-robin order, and the child stayed there. A spawn, a completion, and a
queue hand-off each crossed cores, and a worker that drew the long jobs ran
them alone while the others idled. Each task also mapped and unmapped a fresh
768 KiB guarded stack. On a 4 vCPU Linux container a spawn-then-join cost
119 microseconds on 4 workers against 22 on one, and 16 uneven CPU jobs took
686 ms where 229 ms was possible.

## Decision

- **A task binds to the worker that first runs it.** Its coroutine stack and
  frames never move. Only a task that has never run, which is only owned
  data, may move. Moving a suspended task is rejected: LLVM treats a
  thread-local address as constant within a function, so a frame that read
  one before a yield could reuse the old thread's address after it.
- **A child starts on its parent's worker.** The newest child waits in a slot
  only that worker runs, so start-then-wait stays on one core. Older
  children wait in the worker's stealable queue.
- **Idle workers steal children that have not run.** A worker with nothing
  to run takes half of another worker's queue, once its oldest child has
  waited 250 microseconds, and runs the first. A newest child its worker has
  not picked up within 20 microseconds can be taken too. A worker that
  queues a child while another is parked wakes one parked worker, unless a
  worker is already searching.
- **Busy workers do not starve their own queue.** While every worker is
  busy, a worker runs one of its own children that has waited 100 ms, at
  most once per 100 ms.
- **Each worker reuses task stacks.** A finished task's stack returns to the
  pool of the worker that handed it out, for the next child of the same
  requested size. While the worker has work the pool keeps up to 1,024
  stacks and 1 GiB of reserved address space; an idle worker trims it back
  to 32 stacks and 32 MiB, 16 stacks per pass. Guard pages stay mapped and
  inaccessible, and a reused stack is not cleared.
- **Recursive calls carry scheduling fuel.** The entry of a function on a
  call cycle spends per-task fuel in native code (4,096 units) and yields
  when it runs out, so deep recursion yields like a loop. The interpreter
  yields at each such entry below a task's first call.
- **Cancellation stays cooperative.** Safepoints do not observe it.

## Consequences

- Scheduling order is still unspecified, and a program cannot observe which
  worker runs a task.
- A long task cannot move after it starts, so a worker that starts several
  long tasks runs them in turn even when another worker frees up.
- The stack pool keeps the touched pages of up to 1,024 stacks resident while
  a worker is busy; they are released when it goes idle.
