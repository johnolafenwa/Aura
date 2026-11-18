# Failure recovery

The recovery API returns a diagnostic to the caller after a failed call.
A recursive call runs as long as its stack has room. ADR-0104 and ADR-0105
record the contracts. The design checkpoint is 33.

## Work

The task fixture covers ordinary and named arguments, defaults, owned
values, generators, concurrent tasks, a stored callable, and cancellation.
Both the MIR interpreter and native executable printed the same 21 lines
on two runs. Six checker fixtures exercise argument ownership, a mutable
parameter, a resource result, and invalid call syntax. Each case has a
corresponding compiler library assertion. Runtime tests exercise the
source position, missing paths and gaps in a long call chain. The
language server checks the recovery signature, inferred result and
diagnostic members.

The executable example in chapter 9 processes settings lines, logging a
bad line while allowing the next line through. The Manual distinguishes
a cancelled wait from a task exit and describes the external effects that
survive a failed call.

## Verification

The separate nested cleanup fixture produced the same 18 lines on MIR and
the Linux native backend. It checks a direct call and the named-argument
path, order of exits, the innermost source location and the recovery
boundary's call chain. A second fixture cancels a task while its recovered
call is waiting. The next cancellation check inside `control.retry` exits
the callee and caller in order. Both native paths and MIR produced the
same six lines and returned `TaskResult.Cancelled`.

All 24 checked listings in the edited pages and all 158 language-server
tests passed. The compiler-backed test covers the recovery signature,
argument names and fields on the inferred diagnostic. Z3 proved the
valid recovered call and returned AU5101 for the invalid precondition.
A freestanding build reported AU2052 at the recovery call. An explicit
768 KiB task stack held seven nested scopes on the local debug interpreter
and 10,178 on a native build. Both ran exactly one exit per entered scope
before reporting AU4005. The CLI regression counts each backend’s own
scopes instead of pinning an implementation-dependent depth.

The scalar and call-chain fixtures exercise an owning match, independent
mutation, two wrappers reaching the same literal assertion and failures
with invocation-specific messages. MIR and Linux native matched across all
65 output lines in the five fixtures. A division by zero while building a
message reported AU4004 on both backends.

All 1,304 runtime library tests passed. The cached-file read test ran on
Linux tmpfs, which supports its nonblocking read. The Linux x86-64 kit
from CI run 44203518506 built a 1,144,456-byte hello-world executable,
down 42,080 bytes with the same size gate.

On the release compiler, ten million successful checked scalar calls
and 100,000 caught failures produced medians of 1.35 ns and 104.49 ns per
call with a literal message, or 5.53 ns and 781.29 ns with an interpolated
message. The caught error retains its source and its own formatted
message. The [benchmark](../benchmarks/failure_recovery/README.md)
records comparisons against Rust and Python and the earlier compiler.

The earlier Ubuntu gate measured 96.45% line coverage and 97.23%
function coverage. The runtime tests now call the portable recovery ABI
with an argument the caller still owns and check its reference count
after success and after a trap. They verify that cancellation crosses a nested native
recovery boundary and that an isolated call releases abandoned work if
its stack allocation fails or it yields outside a generator. All 1,351
instrumented runtime library tests passed on Linux. Compared with the
same run of the earlier 1,347 tests, LLVM recorded 22 more functions and
185 more lines in the three runtime files containing these paths.

The 3,000-task readiness test exceeded its deadline on macOS and on a
shared Linux worker. Tasks now share their parent's immutable MIR program
and function indices. The test samples thread usage while all 3,000 tasks
are waiting at the readiness barrier.

## Stack space for cleanup methods

The ARM64 Linux and macOS CLI tests found a recursive task that registered a
`close` method but skipped it when the next call exhausted the stack. The
interpreter applied the same headroom check to the exit while the frame that
owned it was still on the stack. Its failed call left the original trap primary.

A function that can register an Aura exit now reserves space to call that
exit. The registered exit spends that space and keeps the ordinary guard for
its own execution. On x86-64 Linux, requesting 256 KiB through 1 MiB in
4 KiB steps exposed a missing close at 105 of 193 sizes in the earlier
packaged release. The corrected debug interpreter closed every entered scope
at all 193 sizes for a plain resource, a manager, and an exit that opens
another resource. At the original 768 KiB it entered and closed four nested
scopes. The number of frames depends on the build and platform.

On the scoped recovery release at `1dba8ad1`, the same recursive resource
missed one close at 17 of 33 task stack sizes from 256 to 384 KiB. The
debug build at `3d2ae363` missed one at five sizes in that range.
