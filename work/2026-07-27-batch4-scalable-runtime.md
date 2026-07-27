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

B4.0 implementation and its repository gates are complete:

- B4.0-a uses a short cross-process runtime-identity lock plus per-content-key
  writer locks. N concurrent cold runs of one program now produce one build
  and N-1 verified consumers; optimistic established hits do not wait for the
  key writer. The installed immutable-runtime path remains usable with caching
  disabled or unavailable.
- B4.0-b flushes `aura: waiting for a concurrent build...` before a human-mode
  process blocks and `aura: rebuilding native runtime...` before rebuild work.
  Each notice is deduplicated per invocation. JSON mode provisionally buffers
  those exact strings so stderr remains one JSON document: success uses the
  `progress` array and failure uses diagnostic `notes`; an `auto` fallback
  also preserves its direct-to-MIR transition and reason.
- B4.0-c capability-aware AU3001/AU3002/AU3003/AU3005 guidance is committed at
  `4f0461e`. The same clone-safety helper also corrects AU3006's directly
  non-cloneable `random.Rng` wording as a provisional diagnostic gap-fill.
- B4.0-d gate-condition suite-count precision is committed at `5cb4476`.

The Phase 5.1 runtime implementation has not started. The dedicated benchmark
harness is committed at `850e906`, and the contractual before-reactor baseline
is recorded in `work/2026-07-27-phase5-runtime-benchmarks.md`.

## Verification

- The five `native_run_cache_*` tests pass under default test parallelism.
- The complete CLI integration suite passes under default parallelism:
  274 passed, 0 failed.
- The complete default-parallel Rust workspace gate passes, including 931
  compiler unit tests and every integration, fixture, package, scheduler-model,
  and documentation test.
- The deterministic contention regression holds the exact content-key lock,
  starts four processes, observes the flushed wait line while all four are
  blocked, then proves exactly one rebuild, four successful program results,
  one published entry, and a subsequent verified hit with `CC` and `CARGO`
  deliberately unavailable.
- Focused behavior tests also pin unrelated and same-key warm hits while a key
  lock is held, one-document JSON failure, buffered JSON wait progress,
  automatic-fallback reporting, and an uncached installed direct run.
  The timed warm-hit regression uses the installed immutable-runtime fixture,
  so parallel Cargo activity cannot change the runtime archive identity while
  the test deliberately holds the exact cache-key lock. Production runtime
  identity remains strict and content-derived.
- Broad serialization was removed from `npm run test:rust`. The instrumented
  compiler-coverage wrapper retains single-threaded libtest execution because
  the default-parallel probe passed every behavior test but undercounted
  function coverage at 96.86%. The serialized run restored the stable result
  to 4201/4336 functions (96.886531%) while retaining the 15 known LLVM
  mismatched-profile warnings. Dedicated forced-backend parity,
  scheduler-stress, and sanitizer runners also retain their narrow ordering
  constraints.
- The behavior-focused closure tests pin the exact AU2999 enum/literal match
  diagnostic and canonical editor inference for a specialized user generic
  class. The latter raised covered functions from 4201 to 4202 without a
  synthetic execution-only test. Final exact compiler coverage is
  64670/67265 lines (96.142124%), 4202/4336 functions (96.909594%), and
  94996/100674 regions (94.360014%), clearing the frozen
  96.13/96.89/94.35 floors.
- The exact final-tree `npm run ci` gate is green: format; the default-parallel
  Rust workspace; the 529.82-second forced MIR/direct fixture matrix; all 79
  language-server tests with 100% coverage; all 13 extension tests; compiler
  coverage; reference integrity and the retired-syntax sweep; docs build; npm
  and Rust audits; Clippy with warnings denied; and hygiene. The Rust audit
  retains the existing allowed `rustls-pemfile` unmaintained warning.
- This checkpoint change lands B4.0-a/b and its behavior-focused coverage
  closure as one isolated commit family.

## Follow-up

Begin the reactor stage with failing lifecycle/model tests. Every Phase 5 stage
must land independently with behavior, parity, reference, benchmark, coverage,
and cleanup evidence appropriate to that stage.
