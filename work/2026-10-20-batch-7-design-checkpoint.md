# Batch 7 Design Checkpoint

## Goal

Bring Batch 7's key decisions to John before any Batch 7 code starts: the
native backend, inlining, bounds checks, and SIMD. John asked on 2026-10-19
for the work to continue through Batch 6 to these decisions.

## Work Completed

- `architecture_docs/22-batch-7-design-checkpoint.md` gathers the measured
  starting point, the constraints every option keeps, the options for each
  decision with a recommended default, measurement work, and a ten-step
  delivery plan.
- The recommendations: keep Cranelift and finish the backend boundary,
  make frames cheap before a MIR inliner that keeps frame records, keep
  every bounds check observable and remove only proven ones, and vectorize
  only inside the current Array contracts.
- `architecture_docs/08-native-codegen-and-runtime.md` no longer describes
  the retired MIR-runtime launcher fallback for `--backend auto`.
- The roadmap's Batch 7 row links the checkpoint.

## Verification

- Every number in the checkpoint comes from the Performance chapter, the
  backend boundary chapter, ADR-0064, or the Batch 5 and 7a checkpoints.
  The checkpoint marks the two figures nobody has measured: the LLVM
  compiler size and inkwell support for LLVM 22.
- Documentation checks only. The checkpoint changes no code.

## Follow-Up

- John approved all four recommendations on 2026-10-20.
- Step 1 adds Rust baselines for the collection programs and compile-time
  measurements, then proposes numeric targets to ratify.
