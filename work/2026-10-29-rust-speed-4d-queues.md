# Rust Speed Step 4d: Tasks And Queues

Date: 2026-10-29

## Problem

`task_pool` ran in 2.2 s against Rust's 0.22 s (9.9x). Per job it made
about 32,000 instructions for ten queue hand-offs and two small
computations. Each hand-off:

- built a `QueueReceive` variant with a payload vector, tagged it with its
  generic name (a `memchr`, a compare, and a new name each time), tested
  it by name, and took the payload out;
- built and tagged a `Result.Ok(Unit)` for a `put` nobody read;
- locked each queue's sender or receiver registry and SipHashed the task
  into it on every operation;
- yielded after every receive that left values behind.

With four workers the program ran slower than with one: every hand-off
between workers wrote an eventfd, even to a worker that was running, and
an idle worker went straight to `epoll_wait`, so each pipeline stage slept
and woke per value.

## Changes

- Receive slots (`native_codegen_lookups.rs`). A compiler temporary
  assigned from `get`, `GetInTaskGroup`, or `GetWithRegisteredProducers`
  and matched only on `QueueReceive.Item` (other arms may go where the
  match goes without them, as a `for` loop's `Closed` and `Cancelled` arms
  do) holds the item's box, or null. The `_item` runtime helpers return
  exactly that.
- Discarded `put` slots: a temporary assigned from `put` and only
  evaluated or released holds null, and `aura_direct_channel_send_discarded`
  sends without building a `Result`.
- Each task remembers up to eight queue roles it registered, so later
  operations skip the registry.
- A receive that leaves values behind yields, unless one task made the
  last 32 receives from the queue: then it yields every 32 receives.
  Consumers that alternate keep strict turns.
- The scheduler skips its condition-variable notify when no thread waits
  on it.
- The reactor inbox records whether its poller is blocked. A notification
  writes the eventfd only then; a running reactor drains its inbox before
  it blocks. An idle worker spins for up to 50 microseconds on an atomic
  flag before it blocks, unless a descriptor wait is armed (descriptor
  readiness comes only from the poller, and spinning there doubled
  `kv_server`'s time).

## Measurements

Wall time at the bench size, Linux container (4 cores), median of 7:

| Program | Rust ms | Before (Aura / Rust) | After (Aura / Rust) |
| --- | ---: | ---: | ---: |
| `task_pool` | 219 | 9.9x | 2.5x |
| `task_pool`, `AURA_WORKERS=1` | 219 | | 1.4x |

Callgrind instructions at check size, base = step 4c. All 30 programs
print the same output.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `task_pool` | 39.5M | 23.5M | -40.6% |
| `http_api` | 19.2M | 19.4M | +0.7% |
| `kv_server` | 40.8M | 41.0M | +0.4% |

No other program moves by more than 0.3%. `task_pool` with one worker at
5,000 jobs: 161.6M instructions before, 104.6M after.

## Gate

4d's gate (at most 1.5x Rust) is missed with the default four workers and
met with one. What remains is moving each value between cores: tasks are
pinned to the worker they start on and never stolen (see
`docs/manual/execution-model.md`), so a pipeline whose stages land on
different workers pays a cross-core hand-off per value. Closing that needs
a scheduler that moves or co-locates communicating tasks, which is beyond
this step.
