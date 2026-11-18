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

A combined FFI fixture checks recovery on either side of a C callback.
Both backends report AU4001 after C marks its cleanup complete. Recovery
inside a callback lets C continue and return its original value. A
runtime regression covers pending callback state between recovered calls.

A recovered call can accept and return a numeric value whose inline
representation owns a heap reference. The native compiler transfers
these values through the same boxed call boundary as an owning union.
The regression covers small integers, overflow into a large integer,
and a decimal with a large coefficient.

A recovered call can return a callable that captures owned state. MIR
records its specialized call signature on the successful Result payload.
Native recovery uses the generic call boundary when the callee's opaque
ABI still has type parameters. The named, stored, and generic factories
each return callable values that produce the same results on both backends.

A matched integer from `sync.Map` retains its value when the map changes
before `control.recover` receives the integer. The combined fixture checks
missing keys, a negative captured value and zero. MIR and the Linux native
compiler agree on the error message and both subsequent map reads.

A task and the generators it resumes share the compiled MIR program.
Each recovered call starts a fresh call stack and uses the current task’s
cancellation context. A fixture enters a generator from a spawned task,
recovers a failure inside it, checks that the trace contains only the
failed function, and resumes the generator to completion.
