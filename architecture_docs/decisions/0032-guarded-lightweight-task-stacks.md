# ADR-0032: Guarded lightweight-task stacks

- Status: Accepted
- Date: 2026-07-27
- Accepted: 2026-07-29 (Batch 4 checkpoint)
- Roadmap decision: Batch 4, Phase 5.4

## Context

Aurora's lightweight tasks originally requested a fixed 1 MiB coroutine
stack. That conservative size was introduced after URL parsing, TLS
handshakes, HTTP parsing, and WebSocket framing overflowed an experimental
256 KiB stack. Phase 5.4 moved the deep protocol frames to service threads, but
integration then proved that 256 KiB was still unsafe as the global default:
the maintained HTTP example, executed as a complete compiled Aurora workload
with its language-execution frames, terminated with `SIGBUS` when every task
used the experimental 256 KiB default and succeeded when that default was
512 KiB.

That whole-program result is deliberately distinct from the isolated runtime
regressions. A direct Rust runtime round trip can force its protocol-calling
children to 256 KiB and succeed because the protocol service keeps its
representative deep host-library frames on 2 MiB worker stacks. That proves
the offload boundary; it does not include the MIR/direct language-execution
frames of the compiled example and therefore does not establish 256 KiB as a
safe global default. Reserving 1 MiB for every task nevertheless makes the
virtual address-space cost grow much faster than the task's ordinary
resident-memory working set.

The deep protocol frames are not intrinsic to an Aurora task. They can run as
owned, bounded protocol jobs on service threads while the lightweight task
waits through the scheduler. Descriptor readiness, absolute deadlines,
cancellation observation, and task wakeup remain scheduler responsibilities.

Aurora also needs an escape hatch for an application with a genuinely deep
task-local call shape. Adding a `stack_size=` option to `TaskGroup.start` or
`start_soon` is not safe: those methods forward target arguments, so the option
would collide with a target parameter of the same name.

## Decision

- `TaskGroup.start(...)` and `TaskGroup.start_soon(...)` use a default requested
  writable stack capacity of 524,288 bytes (512 KiB).
- Every lightweight-task stack is guard protected by the platform stack
  allocator. The requested writable capacity is rounded up to the host page
  size; guard-page reservation is additional to that writable capacity.
- The collision-free override methods are:

  ```aurora
  start_with_stack(bytes: int64, function, own ...) -> Task[T]
  start_soon_with_stack(bytes: int64, function, own ...) -> None
  ```

- `bytes` is evaluated once before the callable target and its captured
  arguments are started. It is an exact byte count with an inclusive accepted
  range of 262,144 through 67,108,864 bytes (256 KiB through 64 MiB).
- The 256 KiB lower bound is an opt-in minimum for a task whose shallow stack
  use has been measured. It is not Aurora's generally safe default. Programs
  should retain the 512 KiB default unless measurement justifies a custom
  request.
- A value below the minimum or above the maximum is rejected. Aurora does not
  silently clamp it. The allocator may round an accepted value upward to the
  host page size.
- The override changes only the child's stack capacity. Target resolution,
  capture ownership, result typing, group membership, cancellation, failure
  observation, and MIR/direct behavior are otherwise the same as `start` or
  `start_soon`.
- Deep HTTP, TLS, and maintained Unix WebSocket protocol work is moved off
  coroutine stacks onto a distinct bounded protocol-step pool whose service
  threads have deep native stacks. Each submitted operation owns its protocol
  state and performs
  one bounded, nonblocking library step. The coroutine waits for that step to
  return its state before observing cancellation or waiting for descriptor
  readiness, so cancellation cannot strand the moved state or permit a second
  owner to race it. Absolute deadlines, cancellation checks, descriptor
  readiness, and task wakeup remain on the scheduler/reactor side.
- The Phase 5.4 service has two named workers with 2 MiB native stacks and a
  64-job queue. HTTP service steps cover URL/host/request preparation, response
  construction, request/response head parsing, and chunk decoding. TLS service
  steps cover PEM parsing, root/config and rustls object construction, every
  nonblocking handshake transition, reads, writes, HTTPS TLS I/O, and close
  notification. Maintained Unix WebSocket service steps cover URL/request
  construction, every handshake transition, frame reads/writes, and close.
  Non-Unix WebSocket compatibility paths remain on their legacy execution
  paths and are not evidence for the coroutine stack reduction.
- The protocol-step pool is process-global, initialized lazily, and shared by
  all lightweight schedulers. Its workers intentionally live until process
  exit. Aurora 0.1 exposes no runtime shutdown or join operation; idle workers
  wait on the pool condition variable, and closing a `TaskGroup` does not tear
  down the process service.
- The dependency-owned recursive parser used by dynamic `json.parse` is
  isolated from coroutine stacks on a separate, process-global codec service.
  It has two named workers, each with a 2 MiB native stack, and admits at most
  two operations in total across reserved, queued, running, and
  result-publication states. This service is distinct from both the
  protocol-step pool and the generic blocking-I/O pool.
- The codec-service boundary is deliberately narrow. The legacy
  `json.is_valid` and `json.parse_string_map` compatibility helpers retain
  their existing bounded validation/flat-map contracts and execute on the
  caller's path; they do not reserve codec-service capacity or run on its
  workers. `json.stringify_map` likewise remains a caller-side compatibility
  helper.
- Codec capacity is reserved before Aurora makes the fallible owned copy of
  the parse source. A saturated lightweight task parks through a
  scheduler-aware availability notification instead of spinning; a caller
  outside a lightweight task waits on the service condition variable. The
  reservation is released exactly once on source-copy failure, worker panic,
  ordinary codec failure, or successful result publication.
- The dependency-owned recursive parse runs on the codec worker. Conversion
  into the Aurora runtime tree, JSON-aware runtime cloning and rendering, and
  deterministic dumping use iterative traversals so a valid depth-128 value
  does not recreate those recursive frames on a 512 KiB coroutine stack.
- `json.parse` retains its synchronous observable contract. Once admitted, a
  task waits for the codec result; cancellation is deferred until the codec
  operation has completed and is observed at the task's next ordinary
  cancellation boundary. The direct backend copies the source while holding
  its value-table read access briefly, then releases that access before
  submitting or waiting for the codec result.
- The Batch 4 checkpoint accepted this API after reviewing its diagnostics,
  both-backend behavior, protocol cleanup tests, and measured memory results.

The 512 KiB default is a capacity request, not a promise that every task
commits or touches 512 KiB of resident memory. Conversely, dividing whole
process peak RSS by task count is not an incremental per-task measurement.

## Consequences

Ordinary programs keep the compact `start` and `start_soon` calls. Programs
with a measured task-specific requirement can opt one child into a custom
guarded stack without reserving a keyword that may belong to the target
function. Requests below the 512 KiB default are for measured shallow tasks,
not a general memory-saving recommendation.

Page rounding makes the actual writable mapping platform-dependent by less
than one page. Guard-page layout is also platform allocator behavior and is
not usable task capacity.

Protocol work no longer forces every coroutine to reserve enough space for the
deepest third-party parser or handshake frame. A distinct pool prevents these
jobs from occupying the generic blocking-I/O workers, while bounded,
nonblocking steps prevent a stalled peer from holding a protocol worker
indefinitely.

Plain socket syscalls and reactor readiness remain scheduler-side. Resolver,
listener-bind, and file-read work uses the generic blocking-I/O pool. TLS asset
bytes are therefore read there; only subsequent PEM parsing and rustls
construction move to protocol workers.

Recursive dependency frames in dynamic `json.parse` likewise no longer
determine the coroutine-stack default. The separate two-operation codec
admission bound also bounds owned dynamic-parse source copies awaiting
service. The legacy compatibility helpers remain caller-side. Iterative
runtime-tree operations keep the remaining dynamic JSON work on the calling
task without making its host stack depth proportional to JSON nesting.

The clean committed Mac14,9 benchmark at `0dddb43` records 205,389,824 bytes
of worst whole-process RSS and 197,836,800 bytes of worst same-process
incremental RSS for 10,000 parked sleepers. The latter is an amortized upper
bound of 19,784 bytes (19.32 KiB) per requested sleeper, including scheduler
metadata and shared workload growth rather than only stack pages.

The 100,000-sleeper plus 1,000-timer gate remains unavailable under the
benchmark escape hatch: worst whole-process RSS was 1,978,384,384 bytes
against the 1.5 GiB ceiling, although the timer arm span and p99 both passed
at 3 ms. This host has 16 KiB pages, and one resident page for each of those
101,000 stackful child coroutines alone requires 1,654,784,000 bytes before
task metadata or the root runtime. Halving the demand-paged virtual stack
reservation therefore cannot make that RSS ceiling robust. No
massive-concurrency claim follows from this decision.

## Completion tests

- Semantic tests pin both exact signatures, exact `int64` typing, callable
  binding after the size argument, and unambiguous forwarding of target
  arguments.
- Check-fail and run-fail fixtures pin values below 256 KiB and above 64 MiB,
  including dynamic values, without clamping.
- MIR and direct tests pin both handle-returning and handle-free starts and
  transport the same requested capacity to the scheduler.
- Stack-allocation tests pin page rounding, guard protection, and allocation
  failure diagnostics.
- The complete compiled Aurora HTTP workload and recursion tests pin 512 KiB
  as the safe default. Separate isolated runtime regressions force
  protocol-calling children to 256 KiB to prove that deep protocol frames stay
  on service workers; those regressions do not exercise the compiled
  language-execution frame stack. A shallow explicit-stack fixture pins
  256 KiB only as the accepted opt-in minimum.
- Maintained HTTP, TLS, and WebSocket round trips run from a default-stack task
  on both backends, with timeout, cancellation, malformed-peer, cleanup, and
  scheduler-progress regressions.
- Depth-128 JSON arrays and objects pass through dynamic `json.parse`, clone,
  render, and dump from a default-stack task on both backends; depth 129
  retains the exact typed `json.Error` contract. Saturation tests pin
  two-operation admission,
  reservation-before-copy, scheduler-aware progress, deferred cancellation,
  direct value-table lock release, and exact release after failure or panic.
- Recursion regressions still produce Aurora's maintained depth diagnostic
  before a task reaches its guard page.
- Compiler-service and language-server tests pin completion and hover text for
  both override methods.
- The scalable-runtime runner records same-process baseline RSS, parked RSS,
  incremental bytes per task, total 10,000-task peak RSS, and the combined
  100,000-sleeper timer/RSS result. The clean `0dddb43` report publishes the
  measured 10,000-task cost and records the explicit massive-concurrency
  escape-hatch result without turning it into a product claim.
