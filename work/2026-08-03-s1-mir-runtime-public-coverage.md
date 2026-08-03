# S1 MIR-runtime public coverage

## Goal

Close additional `mir_runtime.rs` coverage gaps only through observable behavior
reachable from valid checked Aura source, while removing a duplicated defensive
dispatch arm that checked MIR cannot emit.

## Current baseline

A fresh instrumented `aura-compiler --lib` run passed all 1,621 tests. Before
this slice, `mir_runtime.rs` measured 91.91% lines (662 missed), 92.68%
functions (36 missed), and 88.40% regions (1,571 missed). The whole compiler
measured 95.68% lines, 96.49% functions, and 94.19% regions.

## Observable public behavior

The new regressions pin:

- `list.remove` absence as AU4008 with the actionable membership-check help;
- missing dictionary indexing as AU4003 at the exact source span;
- successful mutable `set.clear` writeback through its empty result;
- the canonical `io.write` and `io.flush` fixture's exact MIR output; and
- failed module-constant initialization stopping `main`, retaining the AU4004
  division diagnostic, and preserving partial initializer output.

A focused LLVM comparison reached 38 previously missed unique region
coordinates across 21 production source-line starts.

## Defensive dispatch removal

The `evaluate_string_method` `to_bytes` arm was unreachable from checked MIR.
The lowerer emits the dedicated `str.to_bytes` host call for literal and
temporary receivers, while place receivers use the borrowed fast path in
member dispatch. Existing regressions pin all three shapes and allocation
failure behavior. Removing the duplicate arm changes the compiler source
denominator by seven lines and eleven regions, including the same reduction in
`mir_runtime.rs`; function count is unchanged.

No malformed MIR or coverage-only runtime value was constructed.

## Verification

- fresh instrumented `cargo llvm-cov -p aura-compiler --lib` baseline: 1,621
  passed;
- focused public-source tests under LLVM instrumentation: 3 passed;
- focused string-byte lowering, borrowed-place, and literal allocation tests;
- full `mir_runtime::tests` suite;
- warning-denied compiler-library Clippy;
- formatting and diff checks.
