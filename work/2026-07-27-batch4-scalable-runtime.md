# Batch 4: Phase 5 scalable runtime

Date: 2026-07-27. Entry commit: `1c249ab`.

## Goal and boundary

Close B4.0's four independent-verification findings, then implement Phase 5
in its ratified strict order:

1. persistent reactor registrations, timer heap, and direct wakeups
2. public `yield_now()`
3. compiler-inserted loop-backedge safepoints
4. coroutine stack reduction after deep protocol work moves to service threads
5. removal of aliased mutable scheduler access
6. structural task-boundary Transfer rules and single-consumer task results
7. pinned-worker multicore execution
8. typed heterogeneous select
9. configurable blocking pool
10. native Aurora/task-ancestry frames and structured diagnostic frames

Stop at the Batch 4 checkpoint. Do not begin Phase 6.

The provisional-decision protocol, reference-freeze discipline, and
behavior-focused coverage rules remain in force. Compiler coverage floors are
frozen for this batch at 96.13% lines, 96.89% functions, and 94.35% regions.
They will be re-ratcheted once, using truncated values, at the checkpoint.

## Benchmark host

- Model: Mac14,9
- CPU: Apple M2 Pro
- Logical CPUs reported by the host: 10
- Memory: 16 GiB
- Operating system: macOS 26.5.2 (25F84)

Contractual benchmark runs must use a dedicated quiet-machine protocol and
record command, build profile, repetitions, validation, peak RSS, CPU, and
latency distributions. The Mac14,9 baseline is calibration evidence rather
than a portable language guarantee until the checkpoint accepts it.

## Entry verification

- Batch 3 is accepted at `1c249ab`.
- The worktree was clean at Batch 4 entry.
- The frozen floors in `scripts/coverage-compiler.sh` are exactly
  96.13/96.89/94.35.
- `target/` was 7.8 GiB with 192 GiB free at entry, below the cleanup
  threshold.

## Current work

B4.0 is in progress:

- reproduce and close direct native-cache cross-process contention
- expose rebuild and concurrent-wait status on stderr
- correct capability-aware AU3001/AU3002/AU3003/AU3005 guidance
- qualify historical suite counts by their gate conditions

Phase 5 implementation has not started.

## Verification

Pending B4.0 focused gates, default-parallel cache tests, full CI, and the
first isolated commit family.

## Follow-up

After B4.0 is committed, establish the before-reactor benchmark baseline and
begin reactor work. Every Phase 5 stage must land independently with behavior,
parity, reference, benchmark, coverage, and cleanup evidence appropriate to
that stage.
