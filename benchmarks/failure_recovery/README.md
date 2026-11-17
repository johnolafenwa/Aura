# Failure recovery

Run `bash benchmarks/failure_recovery/run.sh` after building Aura with
`cargo build --release --locked -p aura`. The script builds the native
Aura and optimized Rust programs, then rotates through Aura, Rust and
CPython for five runs. Set `AURA` to another compiler,
`AURA_BENCH_REPEATS` to change the sample count, or `AURA_BENCH_RUNNER` to
the executable that should launch the measurement process.

Each program loads signed integers from the same 256-element table.
The success loop makes one million checked calls and sums their results
to 127,639,971. The trap loop makes 100,000 calls with negative inputs
and counts one failure per call. Loading the inputs from a collection
keeps each check dependent on the value read at runtime. Compilation,
table construction, output and checksum validation are outside the timed
loops. Rust uses `catch_unwind` with an empty panic hook. Python uses
`try` and `except AssertionError`. Aura matches `control.recover`,
including its structured error value.
