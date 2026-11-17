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

The separate nested cleanup fixture produced the same 17 lines on MIR and
the Linux native backend. It checks a direct call and the named-argument
path, order of exits, the innermost source location and the recovery
boundary's call chain. A second fixture cancels a task while its recovered
call is waiting. The next cancellation check inside `control.retry` exits
the callee and caller in order. Both native paths and MIR produced the
same six lines and returned `TaskResult.Cancelled`.

All 24 checked listings in the edited pages passed. The compiler-backed
editor test passed after failing on the missing completion. Z3 proved the
valid recovered call and returned AU5101 for the invalid precondition.
A freestanding build reported AU2052 at the recovery call. An explicit
768 KiB task stack held seven nested scopes on the local debug interpreter
and 10,178 on a native build. Both ran exactly one exit per entered scope
before reporting AU4005. The CLI regression counts each backend’s own
scopes instead of pinning an implementation-dependent depth.

Release comparisons and the compiler and runtime library suites are in
progress. The benchmark in
`benchmarks/failure_recovery/` times successful calls and traps separately
and checks the outcomes in all three languages.

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
