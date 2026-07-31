# Why Aurora

Aurora 0.2.0 is a technical preview of a statically typed, compiled language
for agent control planes. Its current wedge is the combination of:

- **deterministic ownership**: bare access is shared, `mut` is exclusive
  mutation, `own` transfers a value, and an owning scope has a defined cleanup
  boundary;
- **structured concurrency**: a `TaskGroup` owns the child tasks started inside
  its scope, and scope exit accounts for those children; and
- **typed failure**: files, subprocesses, sockets, HTTP operations, retry
  control, and supervisors expose failure as `Result`, `Option`, or a typed
  outcome rather than an implicit exception path.

“Deterministic ownership” describes value access, transfer, and cleanup. It
does **not** describe a deterministic scheduler. Aurora deliberately leaves
concurrent task completion, cross-worker scheduling, and output order
unspecified. See [Ownership And Borrowing](/manual/ownership-and-borrowing),
[Concurrency](/manual/concurrency), and
[Control-Plane Modules](/manual/control-plane) for the normative contracts.

The useful claim is therefore narrow: Aurora is exploring whether familiar
Python-shaped code can make resource lifetime, child-task lifetime, and
recoverable control-plane failure visible in one language contract. It is not
claiming feature parity with Python, general superiority over another
language, or portable benchmark leadership.

## Measured Snapshot

The tables below are measurements of exact programs, not broad performance
claims and not release gates. They were collected from a clean detached
checkout at commit `18c45ac` on one post-reboot Mac14,9 with an Apple M2 Pro
(10 cores) and 16 GiB of memory. The recorded boot was 30 July 2026 at
23:02:25. The comparison interpreter was Xcode CPython 3.9.6; it was **not** a
free-threaded Python 3.13+ build.

For the four protocol workloads, the harness validates an exact `READY`
record, starts the clock when it sends `GO`, and stops at the exact `DONE`
record. Lower is faster. “Aurora / CPython” is the ratio of medians.

| exact protocol workload | Aurora median | CPython median | Aurora / CPython |
| --- | ---: | ---: | ---: |
| naive recursive `fib(30)` | 93.875250 ms | 158.491666 ms | 0.592304 |
| create and join 10,000 tasks | 101.743042 ms | 51.950667 ms | 1.958455 |
| 20-client delayed loopback TCP fan-out | 104.505375 ms | 108.605459 ms | 0.962248 |
| 16-cycle retrying HTTP worker | 429.291292 ms | 520.447791 ms | 0.824850 |

The TCP shape uses 20 pre-bound loopback listeners. Aurora 0.2 does not permit
transferring an accepted `TcpStream` into a handler task (`AU3008`), and using
one listener would have serialized the handlers instead of measuring fan-out.
The task measurement includes creating and joining all 10,000 tasks after
`GO`. The retry measurement executes the same status and delay schedule in
both programs. Those choices make the pairs reproducible; they do not make
them representative of every application.

The V6 integer loops remain whole-process measurements. Startup-adjusted
values subtract a same-repetition startup control and are estimates rather
than directly timed protocol windows.

| exact 10,000,000-iteration comparison | Aurora whole process | CPython whole process | Aurora startup-adjusted | CPython startup-adjusted |
| --- | ---: | ---: | ---: | ---: |
| Aurora `int32` / CPython integer | 36.620333 ms | 321.096625 ms | 31.037083 ms | 295.458959 ms |
| Aurora `int64` / CPython integer | 13.724042 ms | 321.096625 ms | 7.7378125 ms (10/11 valid) | 296.966042 ms (10 aligned pairs) |

Python has one arbitrary-precision integer lane, so the same CPython program is
shown against Aurora's two fixed-width lanes. These numbers do not imply that
all integer work has the same relationship.

Numeric Arrays were measured separately with NumPy 2.0.2 using one million
`float64` elements and 11 paired single-thread observations on the same host.

| exact Array workload | Aurora median | NumPy median | Aurora / NumPy |
| --- | ---: | ---: | ---: |
| fresh owned elementwise add | 1.142461 ms | 0.251602 ms | 4.540751 |
| existing-array sum reduction | 1.150392 ms | 0.174065 ms | 6.608975 |

This is an initial numeric-runtime result, not a claim of NumPy API
compatibility or competitive parity. The [Numeric Arrays](/manual/numeric-arrays)
chapter records the exact Array methodology and limitations.

The release-performance raw evidence has SHA-256
`06cc1223630b1063c8a6806bf590449d6121a3be8d33e8dc1b0ffd17cee93ccb`.
Its SHA-linked summary has SHA-256
`4490e0d169d9a031ae57f04ade772d22169189f71a949356234f529d40e56236`.
The repository benchmark runner records commands, source and binary hashes,
raw observations, medians, dispersion, host inventories, boot identity, and
the environment policy needed to reproduce the result.

## Adjacent Languages

These projects overlap with parts of Aurora's motivation. The distinctions
below describe focus and language contracts, not a ranking. Primary sources
were checked on 31 July 2026.

### Mojo

Mojo is a close neighbor in Python-shaped systems syntax and compiler-tracked
ownership. Its current roadmap centers
[high-performance kernels on CPUs, GPUs, and ASICs, with Python interoperability](https://mojolang.org/docs/roadmap/).
Its ownership documentation gives each value one owner and defines
[default immutable, `mut`, and `var` argument conventions](https://mojolang.org/docs/manual/values/ownership/).

Aurora 0.2 is narrower. It does not claim GPU programming, heterogeneous
hardware support, or Python-library interoperability. Its present center is
the application control plane around agents: scoped child tasks, transferable
messages, typed I/O and process failures, timeouts, retries, and supervision.

### Nim

Nim is a much broader, established systems language. The Nim project describes
it as a [statically typed compiled language combining ideas from Python, Ada,
and Modula](https://nim-lang.org/), with native executables and deterministic,
customizable memory management. Its current documentation recommends
[ORC for newly written code](https://nim-lang.org/2.2.6/mm.html), and its
[typed-threads documentation](https://nim-lang.org/docs/typedthreads.html)
covers shared-heap and explicit thread facilities.

Aurora is not differentiated merely by deterministic destruction—Nim already
has a strong story there. Aurora fixes one smaller integrated contract around
call-boundary capabilities, structurally transferable task values,
`TaskGroup` scope, and typed control-plane APIs. Nim's metaprogramming,
backend, ecosystem, and portability breadth are outside Aurora 0.2's claim.

### Go

Go is the clearest production reference point for simple concurrent service
software. Its documentation defines lightweight
[goroutines and channel communication](https://go.dev/doc/effective_go#concurrency),
treats [errors as values](https://go.dev/blog/errors-are-values), and explains
that the standard toolchain ships a
[tracing garbage collector](https://go.dev/doc/gc-guide).

Aurora shares Go's preference for visible failure and communication, but
chooses a different lifetime contract: non-copy task captures and messages
must satisfy structural `Transfer`, resources have owners, and child tasks are
normally accounted for by the `TaskGroup` that starts them. This is a semantic
positioning statement, not a performance claim about Go; Go was not part of
the Batch 6 benchmark.

### Free-threaded Python 3.13+

CPython has supported an optional free-threaded build since Python 3.13. The
official guide says that this build can run threads in parallel with the GIL
disabled, while some extension modules may
[re-enable the GIL](https://docs.python.org/3/howto/free-threading-python.html).
It remains Python's shared-object, dynamically typed programming model; free
threading changes execution, not Python into an ownership language.

Aurora instead checks ownership and task-transfer boundaries before execution
and gives common control-plane failures concrete result types. That trades away
Python's runtime flexibility and ecosystem compatibility. The CPython 3.9.6
numbers above must not be presented as measurements of free-threaded Python;
no such comparison was run.

## What Aurora Does Not Claim Yet

Aurora 0.2 does not claim production stability, a stable package ecosystem,
general Python compatibility, GPU or accelerator execution, borrowed Array
views, preemptive scheduling, work stealing, detached tasks, deterministic
concurrent output, or portable performance leadership. The technical preview
is evidence that the language wedge is coherent and executable; wider
adoption, more platforms, broader numerical work, and long-running production
evidence remain future work.
