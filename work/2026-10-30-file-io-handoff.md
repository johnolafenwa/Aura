# File I/O Hand-Off

After 0.10.0, John asked why `file_io` and `docdb` were slower than Python.
He first approved running small local file calls inline on the worker, then
ruled it out: one slow call must never stall a whole worker, and the fix
must not be Linux-only, so io_uring was out too. He chose the cheaper pool
hand-off on 2026-10-30.

## Findings

- `file_io` makes about 15,000 file calls at its bench size. Each went to
  the blocking-I/O pool and back for about 38 microseconds on a RAM disk,
  against 8 for Python's direct system calls.
- A pool worker that took a job woke every other pool worker
  (`notify_all`), and every submission woke one even when a worker was
  about to look at the queue. The calling task parked, so its worker slept
  in its poller and had to be woken when the job finished a few
  microseconds later.
- `FileValue::close` and the flush of other files' buffers before each
  file call wrote on the worker thread, and native `fs.exists` ran its stat
  there.
- `docdb` opens 13 files at its bench size. Its gap to Python is `json.Value`
  path walks, not I/O.

## Changes

- `BlockingIoPool`: a worker that finds the queue empty checks it for 50
  microseconds before it sleeps, and counts itself in `sleeping_workers`
  while it does. A submission wakes a worker only when one sleeps, and a
  worker that takes a job wakes another only when jobs are left.
- `poll_blocking_io_result`: the submitting task checks for its result for
  50 microseconds before it parks, yielding to its worker's other ready
  tasks between checks.
- `FileValue::close` on a handle opened for writing, the cross-file buffer
  flush, and native `fs.exists` run on the pool. The process-exit flush
  still writes directly, since the pool may be gone.
- Tests: a task reading a pipe nobody writes, and a task closing a file
  whose buffered bytes a full pipe cannot take yet, each run beside a
  sibling task on a single worker, and the sibling must finish first. The
  close test fails on the old runtime (a watchdog turns the stall into a
  failure after 10 s).

## Results

RAM disk (`/dev/shm`), Linux container, five runs each:

| Program | Before | After | Python |
| --- | ---: | ---: | ---: |
| 20,000 write + read pairs | 1.53 s | 0.34 s | 0.33 s |
| `file_io` (bench size) | 0.76 s | 0.45 s | 0.44 s |

CPU time falls too: the pair probe uses 1.37 s of user and system time
instead of 2.91 s. On the container's overlay disk the runs vary by 2x
from one to the next; `file_io` averages about 1.2 s instead of 1.7 s.
