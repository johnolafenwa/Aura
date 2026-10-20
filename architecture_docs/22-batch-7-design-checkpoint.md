# Batch 7 design checkpoint: Rust-class native performance

Status: **Ratified 2026-10-20 by the owner.**

Date: 2026-10-20. Source baseline: Batch 6 step 8, the reference tool runner
version 2.

This checkpoint plans
[roadmap Batch 7](14-priority-roadmap.md#priority-batches). The roadmap names
four decisions the owner makes before the work starts:

- which native backend carries Aura's release builds
- how calls are inlined
- how bounds checks are kept and removed
- where Aura uses SIMD

Each section states what the code does today, the options, and the
recommended default. [Owner Questions](#owner-questions) collects the four
choices, and the [Ratification Record](#ratification-record) states what the
owner decided. The [delivery plan](#f-delivery-plan) follows those decisions.

The record this checkpoint completes is
[ADR-0064](decisions/0064-native-backend-strategy-and-codegen-boundary.md),
"Batch 7 decision".

## Starting Point

### Where Native Time Goes

Native builds compile checked MIR with Cranelift 0.118 into one object file.
The host `cc` links that object against `libaura_runtime.a`, a Rust static
library built with fat LTO. Generated code and the runtime meet only at the
linker. Every list length, element read, retain, and release is an
out-of-line `extern "C-unwind"` call, and most of those calls run inside
`catch_unwind`.

The measured gaps sit mostly in runtime code, not in the emitted bodies.

| Workload | Aura | Reference | Ratio | Where the time goes |
| --- | ---: | ---: | ---: | --- |
| `fib(30)`, naive recursion | 89.36 ms | Rust 3.01 ms | 29.7x | 49% frame push and pop, 37% other runtime, 3.4% emitted body |
| 10,000 tasks, create and join | 98.69 ms | Rust 3.40 ms | 29.1x | task stacks and scheduling, owned by Batch 8 |
| 10M checked `int32` increments | 24.33 ms | Rust 16.43 ms | 1.48x | 64-bit add, then a separate range check |
| 10M checked `int64` increments | 9.78 ms | Rust 16.34 ms | 0.60x | Cranelift overflow add |
| `Array[float64]` add, 1M elements | 0.249 ms | Rust 0.248 ms | 1.01x | vectorized Rust kernel |
| `Array[float64]` sum, 1M elements | 1.150 ms | Rust 0.859 ms | 1.34x | left-to-right order is a contract |
| `int_list_sum`, 2M `int64` reads | 0.048 s | Python 0.048 s | 1.0x | `vec_len` plus `vec_index_scalar` calls per element |
| `record_field_reads` | 0.15 s | Python 0.007 s | 21x | type tagging and the ownership ledger |

Sources: the [Performance chapter](../docs/manual/performance.md), the
[per-call attribution](15-backend-boundary.md#per-call-cost-attribution), and
the [Batch 7a checkpoint](20-batch-7a-design-checkpoint.md). The control-plane
and Array rows come from an Apple M2 Pro. The collection rows come from a
four-core Linux x86-64 container. No workload has measurements from both hosts.

Four facts shape every decision below.

- **The frame contract dominates calls.** Every generated function calls
  `aura_direct_enter_call_with_frame` and `aura_direct_exit_call`. Entry
  validates the function name and path as UTF-8 on every call, looks up the
  task state through a thread-local map, pushes two frame records, and
  checks the 256-call depth limit. UTF-8 validation alone is 28.5% of
  `fib(30)`. The text is constant data the compiler wrote, so the check
  proves nothing new after the first call.
- **Generated code cannot inline the runtime.** Cranelift objects and Rust
  LLVM bitcode never meet in one optimizer. A list read in a loop pays a
  `vec_len` call for the guard and a `vec_index_scalar` call that checks
  the bounds a second time.
- **Ownership effects are invisible to MIR.** MIR has no retain or release
  instruction. `native_codegen.rs` decides them while it emits, so no pass
  can cancel a retain against its release.
- **No Rust baseline exists for collections.** The Batch 7a programs compare
  against Python only, and no document measures compile time.

### The Backend Boundary Today

[The backend boundary chapter](15-backend-boundary.md) proposes a small
`NativeBuilder` trait between checked MIR and an emitter. None of it exists.
`native_codegen.rs` is 22,064 lines that call Cranelift's `FunctionBuilder`
directly. It still decides ABI flattening, retain and release, trap
selection, and builtin dispatch by member name. A second emitter today would
re-derive each of those decisions and would need its own parity history.

### Constraints Every Option Keeps

These are contracts, not choices. Changing any of them needs its own ADR.

- Checked integer arithmetic with exact traps, under ADR-0002, ADR-0003,
  ADR-0041, ADR-0047, and ADR-0048.
- Bounds traps with exact diagnostics, and one `int64` index domain with
  negative indices, under ADR-0043 and ADR-0044.
- Structured frames, task ancestry, and the depth limit, under ADR-0036.
- Left-to-right floating reductions and the 1,008 frozen Array bit and
  diagnostic cases, under ADR-0041.
- The forced MIR and direct parity matrix.
- Per-function sections and the binary size budget: hello world within 10%
  of 927,888 bytes on Linux and 922,344 bytes on macOS arm64.
- Standalone builds with no Rust toolchain on the user's machine.
- Linux x86-64, macOS x86-64, and macOS arm64 as the supported hosts.

## Goals And Non-Goals

Goals:

- Ratify numeric targets against Rust for calls, integer loops, list loops,
  and record reads, then meet them.
- Make a direct call cost close to a Rust call while keeping ADR-0036 frames.
- Read list elements and lengths without a runtime call in loops.
- Remove only checks the compiler proves redundant.
- Keep compile time and binary size within measured budgets.

Non-goals:

- Task stack reuse, work stealing, and scheduling. Those are Batch 8.
- Strided Array views, broadcasting, and matrix kernels. Those are Batch 10.
- Freestanding targets, `unsafe`, and cross-compilation. Those are Batch 12.
- An unchecked arithmetic or unchecked indexing mode.
- A native `aura run` default. ADR-0031 keeps that a separate decision.

## A. Native Backend

### Options

| Option | What it gains | What it costs |
| --- | --- | --- |
| **A1. Keep Cranelift, finish the boundary** | Fast compiles, no new build dependency, all parity history kept. Fast paths can be emitted inline from a fixed runtime layout | No general optimizer: inlining, loop work, and vectorization are Aura's own passes |
| **A2. C emission through the host `cc`** | GCC or clang optimizes, inlines, and vectorizes. No new tool to install. Cheaper freestanding targets later | Port of a 22k-line emitter. GCC on Linux and Apple clang on macOS give two optimizers to keep in parity. The C compiler's version joins the build identity. The emitted C must contain no undefined behavior, and unwinding through C frames needs `-funwind-tables`. Compiles get slower |
| **A3. LLVM through inkwell** | Strongest optimizer. Could optimize generated code together with runtime bitcode | libLLVM becomes a build dependency of the compiler, and the 16.8 MB `aura` binary grows by tens of megabytes. Sharing bitcode with the runtime needs the same LLVM as rustc 1.95, which is LLVM 22.1.2. Compiles get slower |

The A3 size and the inkwell support for LLVM 22 are inferences. Neither has
been measured or checked.

### Evidence

- 87% of `fib(30)` is runtime code that LLVM already compiles. A new
  emitter does not remove it. ADR-0064 says the same.
- The integer and Array kernels are within 1.5x of Rust today, except
  `int32`, whose gap is one missing 32-bit overflow add.
- The largest open gap that an emitter could close is the runtime call per
  list read and per retain. A1 closes it by emitting those fast paths
  inline. A2 and A3 close it only after the same layout work.
- Executable size is dominated by the runtime. Generated code was about
  40 KB of hello world before Batch 5.

### Recommendation

**A1.** Cranelift stays the release backend through Batch 7. Batch 7 builds
the `NativeBuilder` boundary and the inline fast paths, then measures again
against the ratified targets. If a gap remains that only a general
optimizer closes, the last step prototypes C emission behind the boundary
and brings the measured comparison back to the owner. LLVM stays out unless
that comparison shows C cannot reach the targets.

### Owner Decision

The owner chose LLVM, after the runtime work:

- Phase 1 fixes the runtime costs on Cranelift and ships a release.
- Phase 2 replaces Cranelift with an LLVM emitter through inkwell. Aura then
  has one native backend. Cranelift is removed, not kept as a second
  backend.
- The MIR interpreter stays. It runs `aura run`, and the forced parity matrix
  compares it with the LLVM build.

The reasons are one optimizer on every host, identical output across
machines, full debug information, cross-compilation from one library, and
inlining of runtime code when Aura's LLVM matches the one rustc uses.

## B. Inlining

### Today

No pass inlines anything. MIR has no optimization passes, Cranelift 0.118 has
no inliner, and runtime helpers cannot cross into generated code.

### Options

| Option | Rule |
| --- | --- |
| **B1. Cheap frames, then a MIR inliner that keeps frames** | First make each frame push cheap without changing what it records. Then inline small, non-recursive Aura functions in MIR. An inlined call still pushes its frame record, so traps, ancestry, and the depth limit stay exact |
| B2. Inline without frames | Inlined calls drop their frame records. Traces lose inlined functions, and the depth limit counts only real calls. This needs an ADR-0036 amendment |
| B3. No Aura inliner | Keep calls as they are. Make frames cheaper only |

### Recommendation

**B1.** The cheap-frame work comes first because it helps every call,
inlined or not:

- validate frame text once per program, not on every call
- pass the task state to generated code once, instead of a thread-local
  lookup per call
- store frame records in a preallocated per-task array

The inliner takes functions under a small MIR size budget, never inlines a
recursive cycle, and keeps evaluation order, cleanup order, and
one-observation semantics. Inlining the frame record means a push and pop of
a constant pointer, which costs a few instructions. Runtime fast paths are
section C's work and need no inliner.

### Owner Decision

Cheap frames come first, in phase 1. With LLVM chosen, Aura writes no MIR
inliner. LLVM's inliner does the work in phase 2, under the same rule: an
inlined call keeps its frame record, so traps, ancestry, and the depth limit
stay exact.

## C. Bounds Checks

### Today

`xs[i]` on a list calls `aura_direct_vec_len`, normalizes a negative index,
tests the range, and then calls `aura_direct_vec_index_scalar`, which checks
the range again. A `for` loop over a list knows its length but still pays
both calls per element. `Array` element reads pass the coordinates as a boxed
list and get back a freshly boxed element.

### Options

| Option | Rule |
| --- | --- |
| **C1. Every check stays observable. Remove only proven ones** | Keep one inline check per access. Remove a check only where the compiler proves the index is in range |
| C2. Add an unchecked opt-out | An `unchecked` index operation or a build flag skips checks. This belongs with `unsafe` in Batch 12 |

### Recommendation

**C1.** Concretely:

- Read a list's length and element storage inline, with no runtime call, from
  a fixed internal layout that `aura-core` describes. The layout is private
  to one compiler version and is not an ABI promise.
- Keep one range check per access, and drop the second check inside the
  element read.
- In a `for` loop over a list and in `for i in range(len(xs))` with no
  mutation of `xs` in the body, prove the index in range and drop the check.
- Read `Array` scalar elements with integer coordinates passed directly, and
  return the scalar unboxed.

A trap keeps its exact diagnostic, and MIR and direct builds keep the same
results.

## D. SIMD

### Today

Aura emits no vector instructions. The only vector code is LLVM's
auto-vectorization of the Rust `float32` and `float64` elementwise Array
kernels. Integer kernels and every reduction are scalar. There is no SIMD
type or intrinsic in the language.

### Options

| Option | Rule |
| --- | --- |
| **D1. Vectorize inside the existing contracts, no new surface** | Vectorize Array kernels where the results stay bit-identical. Integer kernels check overflow per lane and rerun the failing chunk in order to report the first trapping index. Reductions keep left-to-right order |
| D2. Add opt-in reassociating reductions | A form such as `sum(ordered=False)` lets floating sums use vector order. This is a new API and a new result contract |
| D3. Add SIMD types to the language | Vector types and intrinsics. This needs layout and target-feature design |

### Recommendation

**D1.** The contracts hold, every existing Array case still pins the results,
and no new surface needs teaching. Release archives keep baseline x86-64 SSE2
and arm64 NEON. D2 and D3 move to Batch 10, where Array v2 adds views and
matrix kernels.

## E. Targets And Measurement

Step 1 measured every workload against Rust on one Linux x86-64 host. The
owner ratified the targets below on 2026-10-20.

### Step 1 Baselines

The host is a shared four-core Linux x86-64 container with Rust 1.95.0 and
Python 3.13. It was not quiet, so the numbers are diagnostic-grade. Each row
is a median of five to eleven runs. The collection rows time the whole
process. The protocol rows time GO to DONE. Raw reports are in
`work/2026-10-20-batch-7-step-1-measurements/`.

| Workload | Aura | Rust | Aura / Rust |
| --- | ---: | ---: | ---: |
| `fib(30)` | 129.6 ms | 3.95 ms | 32.8x |
| 10,000 tasks | 397.3 ms | 6.50 ms | 61.1x |
| TCP fan-out | 120.5 ms | 121.1 ms | 1.0x |
| `int_list_sum` | 43.7 ms | 2.3 ms | 18.9x |
| `string_list_scan` | 120.3 ms | 2.7 ms | 44.8x |
| `dict_build_get` | 321.6 ms | 21.3 ms | 15.1x |
| `set_insert_contains` | 42.1 ms | 6.1 ms | 6.9x |
| `record_field_reads` | 145.0 ms | 9.3 ms | 15.6x |

Process startup is about 2.0 ms for an Aura hello world and 1.2 ms for an
empty Rust program. So the whole-process ratio understates the loop gap for
the two list programs, where Rust finishes in about 1 ms of work.

`aura build` with the direct backend, from an installed runtime:

| Program | Build | `aura check` | Final link |
| --- | ---: | ---: | ---: |
| Hello world | 0.32 s | 0.004 s | 0.23 s |
| Reference agent | 0.62 s | 0.057 s | 0.21 s |
| `unix_tls_roundtrip`, the largest single-file example | 0.40 s | 0.009 s | 0.21 s |

The front end is under a tenth of each build. Cranelift code generation and
the final `cc` link take the rest. `aura build` writes a fresh executable
each time and does not read the native cache, so a rebuild of unchanged
source costs the same as a cold build. Only `aura run` reuses cached
executables.

### Targets

Each target is a ratio to the matching Rust program on the same host, so it
transfers between machines. Phase 1 targets gate the Cranelift release at
step 7. Phase 2 targets gate the LLVM release at step 12.

| Workload | Today | Phase 1, Cranelift | Phase 2, LLVM |
| --- | ---: | ---: | ---: |
| `fib(30)` | 32.8x | 8x | 3x |
| `int_list_sum` | 18.9x | 5x | 2x |
| `string_list_scan` | 44.8x | 10x | 4x |
| `dict_build_get` | 15.1x | 8x | 4x |
| `set_insert_contains` | 6.9x | 5x | 3x |
| `record_field_reads` | 15.6x | 8x | 4x |
| TCP fan-out | 1.0x | no regression | no regression |
| 10,000 tasks | 61.1x | tracked, no target | tracked, no target |

Build and size targets:

- **Phase 1.** Build times stay within 25% of the step 1 numbers.
- **Phase 2.** A reference agent build takes at most 1.5 s on this host, and
  a hello world build at most 0.6 s. Every program stays inside the native
  binaries size gate.

The reasoning behind the targets:

- **`fib(30)`.** About 87% of its time is runtime bookkeeping. If steps 2
  and 3 remove most of it, Aura lands near 4x to 8x. LLVM's inlining and
  register allocation then bring it toward 3x.
- **List loops.** Steps 4 and 5 replace two runtime calls per element with an
  inline read and one check. Step 6 adds integer kernels. Checked
  arithmetic stays, so Rust's unchecked vectorized speed is out of reach.
- **Strings, dicts, sets, and records.** Their time is mostly hashing,
  formatting, and comparison in runtime code. Phase 1 helps them only
  through retain and release removal and cheaper calls. Phase 2 helps
  through runtime inlining.
- **Tasks.** Task creation cost sits in the scheduler, which no Batch 7 step
  changes. It stays measured so a regression shows.

Step 7 re-measures on the macOS arm64 baseline host with the contractual
release runner, as well as on Linux.

## F. Delivery Plan

Each step is one pull request with its tests, docs, and measurements.

### Phase 1: Runtime Costs On Cranelift

| Step | Work |
| --- | --- |
| 1 | Rust baselines for the collection programs, compile-time measurement, and proposed targets |
| 2 | Cheap frames: one-time frame text validation, task state passed once, preallocated frame storage. Done: `fib(30)` went from 32.8x to about 8x Rust on Linux |
| 3 | Explicit retain and release in MIR, and cancellation of matched pairs. Done for loop element copies: a shared `for` loop that only compares, measures, or searches its element reads it in place. `string_list_scan` went from 44.8x to about 12x Rust and `record_field_reads` from 15.6x to about 13x |
| 4 | Inline list length and element reads from a fixed internal layout, with one bounds check |
| 5 | Bounds-check removal in proven `for` loops, and direct `Array` element reads |
| 6 | Vectorized integer Array kernels with ordered first-trap reporting |
| 7 | Re-measurement against the targets, then a preview release on Cranelift |

Steps 2 and 6 do not depend on the others. Step 4 depends on step 3 for the
retain and release it emits inline. Steps 3 and 5 work on MIR, so phase 2
keeps them.

### Phase 2: LLVM As The Only Native Backend

| Step | Work |
| --- | --- |
| 8 | Move the semantic decisions left in `native_codegen.rs` into checked MIR or a shared lowering, so an emitter only translates |
| 9 | LLVM emitter through inkwell, pinned to the LLVM version rustc uses. CI and release builds provide libLLVM. The forced parity matrix, size gates, and per-function sections pass on LLVM |
| 10 | Runtime fast paths and inlining across the runtime boundary, with frame records kept |
| 11 | LLVM becomes the only native backend. Cranelift code and dependencies are removed, and the native cache format, size baselines, and docs are updated |
| 12 | Re-measurement against the targets, then a release |

During steps 9 and 10, both emitters build in CI so parity failures point at
the new emitter. Step 11 ends that.

Two facts need checking at step 9 before the emitter grows: whether inkwell
supports LLVM 22, and how much libLLVM adds to the `aura` binary.

## Owner Questions

Each names the recommended default first.

1. **Backend.** Keep Cranelift and finish the boundary (A1). Recommended,
   because most of the measured gap is runtime code, not emitted code. The
   alternatives are C emission now (A2) or LLVM now (A3).
2. **Inlining.** Make frames cheap, then inline small functions in MIR while
   keeping their frame records (B1). Recommended, because traces and the
   depth limit stay exact. The alternatives drop frames for inlined calls
   (B2) or skip the inliner (B3).
3. **Bounds checks.** Keep every check observable and remove only proven ones
   (C1). Recommended, because no program changes behavior. The alternative is
   an unchecked opt-out (C2).
4. **SIMD.** Vectorize inside the current contracts, with no new language
   surface (D1). Recommended, because results stay bit-identical. The
   alternatives add reassociating reductions (D2) or SIMD types (D3).

## Ratification Record

The owner decided on 2026-10-20:

1. **Backend.** LLVM through inkwell becomes Aura's only native backend,
   after the runtime work. Cranelift is removed in step 11. The runtime
   work ships first in a release built on Cranelift.
2. **Inlining.** Frames become cheap first. LLVM's inliner replaces the
   planned MIR inliner, and inlined calls keep their frame records.
3. **Bounds checks.** Every bounds check stays observable. The compiler
   removes only checks it proves redundant.
4. **SIMD.** Vectorization stays inside the current Array contracts, with no
   new language surface.

Every other rule above is the ratified default. On 2026-10-20 the owner also
approved every numeric target that step 1 proposed, listed in
[Targets](#targets).
