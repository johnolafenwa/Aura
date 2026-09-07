# ADR-0064: Native backend strategy and codegen boundary

- Status: Accepted direction; detailed design pending
- Date: 2026-09-06
- Implementation: Pre-Batch-1 items 1–4 delivered; measurements published with grades and provenance
- Roadmap: Pre-Batch-1 foundations; incremental boundary work from Batch 1;
  release-backend decision in Batch 7
- Related: ADR-0031, ADR-0038, ADR-0041, and ADR-0058

## Authority and scope

The user approved this amendment to the
[priority roadmap](../14-priority-roadmap.md) on 2026-09-06. Cranelift remains
the native backend through Batch 6. MIR execution remains the other maintained
backend. The original amendment scheduled this work; the implementation status
and evidence for items 1–4 are recorded below. No release is prepared here.

## Pre-Batch-1 measurements

The current direct flag setup in
[`native_codegen.rs`](../../crates/aura-compiler/src/native_codegen.rs) sets
`is_pic`, `unwind_info`, `enable_multi_ret_implicit_sret`, and `opt_level=speed`.
The speed flag is pinned by a unit test and subject to the adoption gates below.
Measure its effect with the release-performance, integer-loop, and
numeric-Array harnesses. Publish the effect, hardware/toolchain/commit
provenance, and paired measurements in the
[Performance chapter](../../docs/manual/performance.md) by the 0.3.4
update. Adopt the flag when the forced MIR/direct parity matrix passes;
otherwise revert it and record the failing fixtures as Batch 7 input.

The standalone `benchmarks/rust_baselines/` project supplies Rust counterparts
for fib30, 10,000 tasks, TCP fan-out, the retrying worker, integer loops, and
Array addition/sum kernels. Tokio handles the three concurrent workloads; the
other programs use plain Rust. The programs match the Aura protocols, work, and
provenance rules, including the existing whole-process integer-loop output. Publish paired Aura/Rust medians before
making a backend decision. These baselines ratify no numeric speed target.

The workspace now configures `[profile.release]`, and user programs still link
against the compiler/runtime `libaura_compiler.a`. The pre-Batch-1 update sets
release optimization, LTO, codegen units, symbol stripping, and omitted debug
data, plus dead-stripping flags for user executables. It preserves the
runtime's unwinding panic strategy; no separate debug companion is generated. Record sizes for `aura`, a native hello
world, and the reference agent while preserving standalone execution and
source diagnostics. Runtime/compiler crate separation stays in Batch 5.

## Thin backend boundary

The completed [backend boundary note](../15-backend-boundary.md) inventories
semantic decisions in `native_codegen.rs` and `native_runtime.rs`, identifies
which inputs checked MIR already carries, and proposes incremental shared
lowering behind a small illustrative builder interface. No refactor is made.

Push semantic decisions into checked MIR; make native emission a mechanical
translation behind a small builder interface. The interface's exact shape is
to be designed in Batch 1. A later second native backend should implement
this interface instead of duplicating ownership, cleanup, typing, or failure
semantics. Adopt the boundary incrementally from Batch 1 under parity and
coverage gates. All Batch 1–4 and 6 features must run on both maintained
backends through this boundary, with the forced parity matrix as the gate.

The structural split of `sema.rs` in Batch 1 separates type representation,
capability checking, and place/loan analysis while the type/callable surface
changes. It supports this semantic boundary without switching native backend.

## Batch 7 decision

With Rust baselines and the thin boundary in place, select the release-native
strategy among:

- LLVM through inkwell;
- C emission through the host C compiler already required by native builds;
- continued improvement of Cranelift code generation.

C emission is a serious candidate because it avoids a separate LLVM build
dependency and makes the freestanding targets of Batches 12–13 cheaper to
support. Decide using measured runtime, compilation, code-size, diagnostic,
and toolchain results. No backend switch occurs before Batch 7.

## Why the switch is not first

The measured Array and task gaps involve Rust runtime code already compiled
by LLVM. A new native emitter alone does not resolve those runtime costs.
The language surface is also about to change substantially: duplicating it
across a new backend before the semantic boundary is settled increases work
and parity risk. Cranelift's compilation speed remains valuable: it could
eventually let `aura run` compile natively and retire MIR interpretation.
That possibility is a later measured decision, not a CLI-default change here.

## Completion evidence and locations

| Evidence | Location and criterion |
| --- | --- |
| Release-performance and Rust workloads | `scripts/bench-release-performance.py` and `scripts/test_bench_release_performance.py`; paired fib/task/TCP/retry medians and matching protocol results |
| Integer loops | `scripts/bench-direct-integer-loops.py`; paired equivalent Rust loops and before/after optimization results |
| Numeric kernels | `scripts/bench-numeric-arrays.py` and `scripts/test_bench_numeric_arrays.py`; paired add/sum results with equal workload and allocation accounting |
| Semantic parity | `crates/aura/tests/backend_parity.rs` via `npm run test:backend-parity`; forced MIR/direct matrix green, failing fixtures retained if the optimization flag is reverted |
| Boundary regression coverage | `crates/aura-compiler/src/mir_tests.rs` and `crates/aura-compiler/src/native_codegen_tests.rs`; moved semantic decisions retain observable results and diagnostics |
| Coverage | `scripts/coverage-compiler.sh` and the existing compiler gates; incremental refactors retain required coverage |
| Standalone artifacts and diagnostics | `scripts/test_release_packaging.py` and the existing CLI tests; optimized executables still run and retain source diagnostics; archive symbols remain linkable |
| Published measurements | `docs/manual/performance.md`; provenance, paired medians, and executable-size table |
| Boundary design | `architecture_docs/15-backend-boundary.md`; semantic-decision inventory and small builder-interface design |
| Usability oracle | One current-surface package under 400 lines in `examples/agents/`, included in example smoke tests and run on both backends; before/after diffs at Batch 1–4 and 6 checkpoints |

The reference agent contains a tool registry, typed tool schemas, retries, a
streaming loop, and structured cleanup using only current language/library
facilities. Upcoming generated schemas or generators must not be prerequisites
for its initial version.

## Remaining design

The final builder API, incremental semantic moves, and final backend choice
remain design work for their scheduled batches. Rust source placement and
release-profile values are now fixed by the implementation below. New language or protocol spellings are to be
designed in their owning batch. The ten Approved Decisions remain unchanged.

## Pre-Batch-1 implementation for 0.3.4

Items 1–4 now implement the explicit user decisions from the autonomous task:
Cranelift `speed`; the [boundary inventory](../15-backend-boundary.md); standalone
Rust 1.95.0/tokio 1.53.1 baselines and three runner lanes; and release-profile,
link-section collection, executable stripping, and clean-ref size tooling.

The chosen release profile is level 3, fat LTO, one codegen unit, no debug data,
and stripped compiler symbols, with unwinding retained. User executables collect
unused sections on macOS/Linux and strip debug/local symbols while retaining
globals; the runtime archive keeps linkable symbols. Native-cache format v6
invalidates executables from the previous link/strip construction pipeline
without changing product or semantic-schema versions. Both link steps and the optimization flag pass the local adoption gates:
385 forced parity fixtures, compiler/CLI acceptance, packaging, native-cache
checks, and stripped direct/MIR source diagnostics under the release profile.
Coverage remains above the unchanged floors. Hosted verification is required
before merge. No backend refactor or switch is part of this task.

The 7 September post-reboot session supplies the timing evidence below;
protocol smoke checks establish correctness only. The separate executable-size
measurement records byte counts and hashes at clean refs, including an after
build with Cargo's default release profile restored through environment overrides.
`retrying_network_worker.au` is the reference-agent stand-in until item 6 lands.

The [executable-size table](../../docs/manual/performance.md#executable-size)
records before/default-after/tuned-after builds with hashes and clean-ref
provenance. Tuned executable reductions from v0.3.3-preview are 29.35% for the
compiler, 93.29% for hello world, and 84.54% for the retrying-worker stand-in.
No flag or link step was reverted. Cargo 1.95's reference lists `strip="none"`;
the explicit no-strip size control is retained under the session's conditional
decision, with omitted-setting auto-stripping distinguished in the chapter.

## Post-reboot measurements and Batch 7 inputs

Items 1 and 3 are measured and published in the [Performance chapter](../../docs/manual/performance.md).
The same-session foundations comparison reduces fib/int32/int64 medians by
19.0566%/20.5553%/15.6863%. Task creation changes by -1.6061%, TCP by -0.2547%,
and retry by +0.2398%. Array add changes by +7.1740% and sum by -0.0994%.
All CPython/NumPy controls, including startup, drift by less than 5%; no repeat
was required. The comparison includes release/link tuning with Cranelift speed
and cannot isolate the flag's causal effect.

Contractual Aura/Rust protocol ratios of medians are fib 29.664884, tasks
29.068501, TCP 0.996856 and retry 0.907338. Contractual Array add/sum ratios
are 4.928642/1.338511. Standalone int32/int64 paired ratios are
1.478258/0.594855, diagnostic-grade because that runner lacks the release
suite's full qualification checks. Startup and Rust's per-update optimizer
barrier are included; this is not a general code-generator ranking.

Batch 7 inputs are the large fib/task gaps against these Rust references,
Array runtime/kernel costs, integer-width and startup behavior, the existing
size reductions and parity evidence, and source/measurement qualifications.
TCP and retry results do not support assuming a uniform concurrency penalty.
No backend switch or numeric target is ratified by these measurements.

Initial frozen task/TCP inputs failed AU2002 at both bases. The user approved
identical int64-to-int32 argument casts, preserved in corrected clean after
`4e1e48c81bae6c62a54af763a4046901820038ec` and before
`ddeaddf74301322fc96d8c09742ea12faa1dd8c3` commits. Compiler and runner code
remained unchanged. All Rust protocols/checksums passed; no exclusion was needed.
A verified Git bundle, source diffs, initial failures and successful reports
are retained in the [session evidence](../../work/2026-09-07-foundations-measurements/).
Its manifest SHA-256 is `911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67`.
