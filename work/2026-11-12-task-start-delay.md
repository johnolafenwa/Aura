# Children of one TaskGroup start late on Linux

Date: 2026-11-12. Reported by the Array v2 work, outside its scope: two
CPU-bound children started in one `TaskGroup` took twice as long as one,
84 ms against 177 ms for 20 rounds on the 4-core cloud box.

## Goal

Reproduce the report on `main`, find the cause, and fix it if it is a
defect.

## Work completed

- Reproduced on `main` @ `a6d2baf` with
  `benchmarks/scalable_runtime/parallel_start.au`. The children do run
  on different workers. Each round of two children takes 1 to 2 ms
  longer than a round of one, whatever the length of the child. A child
  of about 1 ms, as in the report, therefore looks sequential.
- Cause: a worker that finds a child too young to steal parks and waits
  `NEWEST_CHILD_STEAL_AGE`, 20 microseconds, in `RuntimeReactor::poll`.
  `mio` passes the timeout to `epoll_wait` in milliseconds, rounded up,
  so the wait took 1.1 ms. The parent's worker runs the newest child and
  the older one sat in the inbox until the thief woke. macOS `kevent`
  takes nanoseconds and is not affected.
- Fix: `RuntimeReactor::poll` spins through a caller's wait shorter than
  `SHORT_WAIT_LIMIT`, 500 microseconds. It polls the platform poller
  with a zero timeout first, and again every 20 microseconds while a
  descriptor wait is armed. A notification ends the spin. Timer-only
  waits with no caller deadline still sleep in the poller.

## Verification

- New reactor tests: a 20 microsecond wait returns in under 500
  microseconds, a short wait ends at a wake, a control notification or
  a timer, and a short wait admits descriptor readiness.
- `cargo test -p aura-runtime --lib`: 1,123 passed.
- 20 rounds on the 4-core cloud box, milliseconds, before and after:

| Child length | one | two | four |
| --- | ---: | ---: | ---: |
| 0.6 ms | 12, 12 | 23, 17 | 36, 20 |
| 3 ms | 56, 57 | 81, 63 | 83, 67 |
| 23 ms | 462, 477 | 484, 472 | 494, 482 |

- The Array v2 shape, four tasks that each sum a shared 2000 by 2000
  Array, on that branch: two children 44 to 33 ms, four 50 to 31 ms,
  one 23 ms.
- `spawn_join 20000`, `imbalance`, `task_pool 100000` and
  `pubsub_broker` are unchanged within run-to-run noise.

## Follow-up

- An older queued child waits `QUEUED_CHILD_STEAL_AGE`, 250
  microseconds, before another worker may take it. That keeps chatty
  tasks on one worker and costs two children about 0.3 to 0.5 ms per
  round over one. A trial at 40 microseconds gained little and was not
  kept.
