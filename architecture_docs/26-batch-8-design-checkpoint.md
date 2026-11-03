# Batch 8 Design Checkpoint: Multicore Runtime

Status: **Ratified 2026-11-03.** The owner approved every default below
("Approve all").

Batch 8 in the [roadmap](14-priority-roadmap.md#priority-batches) covers load
balancing and work stealing, preemption latency, cancellation
responsiveness, wake paths, and task memory. It must also say which
suspended frames may migrate between workers and keep structured cleanup
correct across worker boundaries. The roadmap's
[task-creation acceptance](14-priority-roadmap.md) scopes guarded stack
reuse under [ADR-0032](decisions/0032-guarded-lightweight-task-stacks.md#future-extension-task-stack-reuse).

## Starting Point

- Tasks are stackful corosensei coroutines. Each one gets a fresh 768 KiB
  guarded stack: an `mmap`, an `mprotect` for the guard page, and a
  `munmap` when it ends.
- `LightweightWorkerCoordinator::submit` places every child on the next
  worker in round-robin order. The child is admitted there and stays there.
  There is no stealing, so a worker that got the long jobs runs them alone
  while the others idle.
- A child on another worker means every spawn, completion, and queue
  hand-off crosses cores: a lock on the target inbox, an eventfd write when
  that worker is parked, and an `epoll_wait` return on the other side.
- Per-task native runtime state (frame stack, ownership ledger, cleanup
  stack) lives in a per-worker thread-local map keyed by task id. The
  per-thread box cache and the generated code's frame-stack accessor
  (`memory(none)` on LLVM) assume the thread does not change under a
  suspended frame.
- Loop backedges carry a safepoint: generated code counts 4,096 iterations
  in a register and then calls `yield_now`. Calls carry none, so deep
  recursion without loops never yields. Safepoints do not observe
  cancellation.

### Baselines (Linux container, 4 vCPU, main @ e8d7349)

| Probe | 4 workers | 1 worker |
| --- | --- | --- |
| spawn then join, one at a time, per task | 119 us | 22 us |
| spawn 100,000 then join all, per task | 85 us | 24 us |
| `tasks_10000` release benchmark (whole process) | 0.87-0.97 s | |
| `task_pool 200000` (cross-language) | 1.06-1.53 s | 0.75 s |
| 16 CPU jobs, 4 heavy, round-robin placement | 686 ms | |
| worst 1 ms sleep lateness beside a hot loop | 1 ms | 1 ms |

`strace -c` on 5,000 spawn-joins shows one `mmap`, `mprotect`, and `munmap`
per task, about 2 `futex`, 2 `epoll_wait`, and 2 `write` calls per task.
The 1-worker number is the cost of the task itself; the rest of the 4-worker
number is cross-core hand-off. The imbalance probe's ideal is 11 of its 44
work units per worker; round-robin gives worker 0 all four 8-unit jobs, 32
units, so it runs about 2.9x longer than it needs to.

## A. Which Frames May Migrate

- **A1. A task binds to a worker at its first resume (default).** A task
  that has never run is only owned data: its entry closure, captures,
  cancellation context, and an unused stack. It may move to any worker. Once
  a coroutine has run, its frames stay on that worker until it ends, as
  today. Structured cleanup, forced exit, and ancestry need no new rules,
  because every suspended frame is still resumed and reset on the thread
  that created it.
- A2. Suspended tasks migrate at scheduler waits. Rejected for this batch.
  LLVM treats a thread-local address as constant within a function, so a
  Rust or generated frame that read a thread-local before a yield may reuse
  the old thread's address after it. Making this safe needs an audit of
  every yielding call chain, a move of the per-task runtime state between
  thread maps, and per-thread caches that tolerate it. That is a separate,
  larger project; nothing in A1 blocks it later.

## B. Placement And Stealing

- **B1. Local first, steal what has not started (default).** A child goes to
  the spawning worker. The newest child sits in a private slot that only its
  worker runs, so spawn-then-join stays on one core. An older child moves to
  the worker's stealable queue. A worker with nothing to run takes half of
  another worker's stealable queue before it spins and parks, and a worker
  that adds stealable work while another is parked wakes one parked worker.
  A worker also takes from its own stealable queue every 61 turns so a busy
  ready queue cannot starve it.
- B2. Keep round-robin placement. Rejected: it pays the cross-core cost on
  every spawn and still cannot rebalance.

## C. Stack Reuse

- **C1. A bounded per-worker pool (default).** When a coroutine ends, its
  worker keeps the stack for the next child of the same size, up to 32 stacks
  per worker and 32 MiB of reserved address space. A stack returns to the
  pool of the worker that handed it out, so a worker whose children are
  stolen still reuses their stacks; an `munmap` in a process with threads on
  several cores costs a TLB shootdown on each of them. Guard pages stay mapped
  and inaccessible. A reused stack is not zeroed: no Aura value can read a
  dead frame, and the interpreter's headroom probe uses the stack bounds,
  not its contents. Stacks above the bound are unmapped as today.
  *Amended in 8e:* the busy bound is 1,024 stacks and 1 GiB, and an idle
  worker trims back to 32 stacks and 32 MiB, 16 stacks per pass. With only
  32 kept, a cancelled group of 1,000 parked children unmapped about 970
  stacks, and the TLB shootdowns made cancellation 4x slower on 4 workers
  than on one.
- C2. A process-wide pool. Rejected: one lock on every spawn and exit.

## D. Preemption

- **D1. A budget on calls as well as loops (default).** The fuel counter
  moves from a register into the task's frame stack, which every call
  already touches. Backedges and the entries of functions in a recursive
  cycle decrement it, and `yield_now` runs when it reaches zero, so deep
  recursion yields like a loop does. Straight-line code with neither stays
  as it is.
- D2. Signal-based asynchronous preemption. Rejected: an interrupt can land
  in runtime code that holds a lock or a half-built value.

## E. Cancellation

- **E1. Keep cancellation cooperative; make its delivery prompt (default).**
  Safepoints keep not observing cancellation, so no new exit points appear in
  existing code. Batch 8 measures and fixes how fast `cancel()` reaches a
  task parked on another worker, and keeps `cancelled()` a cheap read so a
  loop can poll it.
- E2. Safepoints observe cancellation and leave the task. Rejected for now:
  it changes the language's cancellation contract and adds an exit point to
  every loop.

## F. Task Memory

- **F1. Measure, then trim the per-task records (default).** Report bytes
  per parked task from the `10k_sleepers` workload before and after, and cut
  what the measurement shows, such as map nodes and per-task boxes. The
  768 KiB stack reservation stays: it is address space, and only touched
  pages are resident.

## Steps

Each step is one PR with its own measurements. 8a and 8d touch different
code and run in parallel; 8b builds on 8a.

| Step | Work | Gate |
| --- | --- | --- |
| 8a | Stack pool (C1), plus spawn probes in `benchmarks/scalable_runtime/` | no stack syscalls on reuse; bounded-retention test; spawn-join faster at 1 and 4 workers |
| 8b | Local-first placement, stealing of unstarted tasks, idle-worker wake (A1, B1) | spawn-join at 4 workers within 2x of 1 worker; imbalance probe within 1.3x of ideal; `task_pool` at 4 workers no slower than at 1 |
| 8c | Wake paths: cheaper cross-worker notification and queue hand-off | `task_pool` and `pubsub_broker` re-measured; no regression in the 30-program suite |
| 8d | Call-and-loop fuel in the frame stack (D1) | deep recursion beside a sleeper wakes the sleeper within 10 ms; fib30 and loop benchmarks within 3% |
| 8e | Cancellation delivery (E1) | cancel-to-finish of a parked child on another worker measured and bounded |
| 8f | Task memory (F1) | bytes per parked task reported before and after |
| 8g | Docs (execution model, ADR-0032 update, a placement ADR), re-measure, roadmap row | all gates reported |

## Results

Linux container, 4 vCPU, all steps together against main @ e8d7349. Wall
times are medians of 3 runs; this container's timings vary by about 10%.

| Gate | Before | After |
| --- | --- | --- |
| 8a: stack syscalls when a stack is reused | `mmap`, `mprotect`, `munmap` per task | none |
| 8b: spawn then join, per task, 4 workers / 1 worker | 119 us / 22 us | 7.6 us / 4.6 us (1.7x) |
| 8b: imbalance probe (ideal 234 ms) | 686 ms | 256-395 ms, usually 1.1x ideal |
| 8b: `task_pool 200000`, 4 workers / 1 worker | 1.06-1.53 s / 0.75 s | 0.60-0.87 s / 0.59-0.74 s: about even, gate not clearly met |
| `task_pool 100000` (suite size), 4 workers | 0.93 s | 0.45 s |
| 8c: `pubsub_broker` / `kv_server` / `http_api` | 4.38 / 3.64 / 0.61 s | 1.37 / 0.83 / 0.49 s |
| 8c: 30-program suite, total Aura time | 26.4 s | 19.1 s; no program executes more instructions |
| 8d: worst 1 ms sleep lateness beside `fib(35)`, 1 worker | 137-187 ms | 1-7 ms |
| 8d: `fib30` instructions | 104,276,818 | 104,273,665 |
| 8e: `cancel()` to last exit, 1,000 parked children, 4 workers / 1 worker | 60 / 15 ms | 8-12 / 7 ms |
| 8f: resident bytes per parked sleeper (10,000) | 7,670 | 7,260 |

Of a parked task's 7.3 KB, 4 KB is the one touched page of its stack. The
rest is the task's records: task state, the coroutine record, the native
runtime state with its 16-entry frame stack, and its wait registration.
