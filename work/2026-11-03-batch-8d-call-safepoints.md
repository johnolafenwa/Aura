# 2026-11-03: Batch 8 step 8d, call safepoints

Batch 8 design checkpoint, decision D1: deep recursion with no loop must
yield to the scheduler like a loop does, so a sibling task on the same
worker is not starved.

## What changed

- `mir::recursion::recursive_functions` finds the functions on a call
  cycle. The call graph is conservative: a function calls every function
  named in its body, and a function with a call the graph cannot resolve (a
  function value, a dynamic member, a builtin, a runtime op, an f-string)
  calls every function that escapes as a value, a method, or a `Display`
  wrapper. An iterative Tarjan walk finds the cycles.
- Native code: `DirectFrameStack` gains `call_fuel`, and its layout
  (and fingerprint) gains `call_fuel_offset`. The entry of each recursive
  function, after its frame push, counts the fuel down and at zero stores
  `DIRECT_CALL_SAFEPOINT_INTERVAL` (4,096) again and calls
  `aura_direct_yield_now`. On x86-64 the fast path is `decq 0x28(%r14); je`.
- Loops keep their register counter. A prototype that moved loop fuel into
  the frame stack had the same instruction count on a 100M-iteration loop
  in a task but ran 5.3x longer (246 ms against 46 ms), because the
  counter's store-to-load chain serializes the loop.
- The MIR interpreter yields at every recursive entry below a task's first
  call. Its scheduler polls
  timers once every 64 ready turns, and a debug-build interpreted call
  costs about 190 us, so any call fuel left timers starved for 100 ms in
  the CLI test.
- A module with no task start still elides every safepoint, so `fib30` and
  `int64_loop` compile to the same code as before.
- Safepoints still do not observe cancellation.

## Measurements

Linux container, 4 vCPU, against main `e8d7349` built in its own target
directory.

| Probe | main | 8d |
| --- | --- | --- |
| `sleeper_vs_recursion.au`, `AURA_WORKERS=1`, worst 1 ms sleep lateness | 137-187 ms (the whole `fib(35)`) | 1-7 ms over 25 runs |
| `fib30` instructions (callgrind) | 104,276,818 | 104,273,665 |
| `int64_loop` instructions | 549,555 | 549,773 |
| `fib(32)` in a task, instructions | 271,990,781 | 288,330,688 (+6.0%) |
| 100M-iteration loop in a task, instructions | 431,694,997 | 431,693,203 |

Wall times of `fib30` and `int64_loop` medians stayed within noise of main
(31 alternating runs: 20.7 against 21.0 ms and 6.9 against 6.7 ms). The
`fib(32)` task is 2 instructions more per call; its wall time moved by
-8% to +6% between passes on a loaded host.

## Tests

- `mir_recursion_tests.rs`: self, mutual, method, and function-value
  recursion; the interpreter's lowering; the Tarjan walk.
- `native_codegen_tests.rs`: a recursive entry calls `aura_direct_yield_now`
  in a concurrent module and not in a sequential one; a function on no
  cycle carries no check.
- `mir_runtime_tests.rs`: interpreter call safepoints run only while tasks
  can run.
- `cli.rs` `recursive_call_safepoints_prevent_timer_and_queue_starvation`:
  a loop-free recursive task beside a 10 ms sleeper on one worker, on both
  backends.
