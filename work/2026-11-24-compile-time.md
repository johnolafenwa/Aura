# Compile-Time Evaluation And Conditional Compilation

The 0.18 integration change adds pure module initialization, required `const` bindings, target properties, package and CLI features, and conditional declarations. The native backend emits immutable data. A freestanding kernel uses its computed decimal lookup table on AArch64 and RV64.

## Verification

The baseline is commit `6642a7c2`. On Linux x86-64, five runs of the release compile-time benchmark gave a median cold hello-world build of 0.371 seconds and a 1,175,872-byte executable. The reference-agent and Unix TLS cold medians were 0.945 seconds and 0.791 seconds.

The initial parse and emission regressions failed on the baseline. The editor runs passed 165 language-server tests and 68 extension tests against a combined compiler. Language-server coverage was 100% for lines, statements, branches and functions. The live debugger test needs a host with LLDB's debug adapter.

All 11 compiler fixture groups passed, including diagnostics, runtime output, debug objects and MIR parity with source locations. The manual and book passed all 1,126 documentation examples. QEMU booted the AArch64 and RV64 kernels and exercised the computed decimal table.

The Linux compiler coverage baseline was 96.53% lines, 97.48% functions and 95.43% regions. The macOS baseline was 96.51% lines, 97.46% functions and 95.42% regions. Both measurements used the same Git tree as the baseline commit. [Baseline workflow](https://github.com/johnolafenwa/Aura/actions/runs/45333109317)
