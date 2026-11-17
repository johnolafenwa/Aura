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

Release comparisons, the compiler and runtime library suites, and the
editor tests are in progress. The benchmark in
`benchmarks/failure_recovery/` times successful calls and traps separately
and checks the outcomes in all three languages.
