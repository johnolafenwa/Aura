# Failure recovery

Run `bash benchmarks/failure_recovery/run.sh` after building Aura with
`cargo build --release --locked -p aura`. The script builds the native
Aura and optimized Rust programs, then rotates through Aura, Rust and
CPython for five runs. Pass `formatted` to repeat the workload with the
failing integer interpolated into each message. Set `AURA` to another compiler,
`AURA_BENCH_REPEATS` to change the sample count, or `AURA_BENCH_RUNNER` to
the executable that should launch the measurement process.

Each program loads signed integers from the same 256-element table.
The success loop makes ten million checked calls and sums their results
to 1,276,406,149. The trap loop makes 100,000 calls with negative inputs
and counts one failure per call. Loading the inputs from a collection
keeps each check dependent on the value read at runtime. Compilation,
table construction, output and checksum validation are outside the timed
loops. Rust uses `catch_unwind` with an empty panic hook. Python uses
`try` and `except AssertionError`. Aura matches `control.recover`,
including its structured error value.

## Results

Median nanoseconds per call on one Intel Xeon Platinum 8573C core, Linux
x86-64, using the release compiler from CI run 44203518506 (feature
commit `1dba8ad1`), Rust 1.95.0 and CPython 3.12.14. Each column uses
five samples. The script checks the checksum of every run.

| Language | Literal success | Literal failure | Formatted success | Formatted failure |
| --- | ---: | ---: | ---: | ---: |
| Aura | 1.35 | 104.49 | 5.53 | 781.29 |
| Rust | 1.39 | 8094.02 | 3.13 | 12556.90 |
| Python | 337.32 | 908.15 | 280.70 | 1851.51 |

The handoff revision (`67b0d91`) took 379.66 ns per success and
12,008.88 ns per failure on the literal workload. After optimizing
literal checks, the same run with a formatted message took 118.48 ns per
success and 5,698.23 ns per failure (`c76ec361`). Returning a checked
scalar result directly also handles the formatted message and preserves
each failing input in its own diagnostic.

These loops count errors without reading their fields. Reading or
changing a diagnostic, recovering a stored callable, and cleaning up
resources exercise other runtime paths. The conformance fixtures cover
their behavior. These timing results apply to the checked scalar calls
measured here.
