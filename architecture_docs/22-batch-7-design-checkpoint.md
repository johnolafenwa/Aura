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
choices. The [delivery plan](#f-delivery-plan) assumes every recommendation.

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

The roadmap ratifies numeric targets only after baselining, so step 1 adds
the missing evidence:

- Rust counterparts for `int_list_sum`, `string_list_scan`,
  `record_field_reads`, `dict_build_get`, and `set_insert_contains`
- compile-time measurement of `aura build` for hello world, the reference
  agent, and the largest example, cold and warm
- one machine for each supported host architecture running every workload

Step 1 then proposes targets, such as `fib(30)` within 3x Rust and
`int_list_sum` within 2x Rust, for the owner to ratify before step 2 starts.
Those numbers are placeholders until the baselines exist.

## F. Delivery Plan

Each step is one pull request with its tests, docs, and measurements.

| Step | Work |
| --- | --- |
| 1 | Rust baselines for the collection programs, compile-time measurement, and proposed targets |
| 2 | Cheap frames: one-time frame text validation, task state passed once, preallocated frame storage |
| 3 | 32-bit overflow arithmetic for `int32` and narrower widths |
| 4 | Explicit retain and release in MIR, and cancellation of matched pairs |
| 5 | Inline list length and element reads from a fixed internal layout, with one bounds check |
| 6 | Bounds-check removal in proven `for` loops, and direct `Array` element reads |
| 7 | MIR inliner for small non-recursive functions that keeps frame records |
| 8 | Vectorized integer Array kernels with ordered first-trap reporting |
| 9 | `NativeBuilder` boundary, then re-measurement against the targets |
| 10 | A C emission prototype behind the boundary, only if step 9 leaves a gap that needs a general optimizer |

Steps 2, 3, and 8 do not depend on the others. Step 5 depends on step 4 for
the retain and release it emits inline.

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

The owner ratified this checkpoint on 2026-10-20 and approved all four
recommendations:

1. Cranelift stays the release backend. Batch 7 finishes the `NativeBuilder`
   boundary, and a C emission prototype follows only if a gap remains that
   needs a general optimizer.
2. Frames become cheap first. A MIR inliner then inlines small,
   non-recursive functions and keeps their frame records.
3. Every bounds check stays observable. The compiler removes only checks it
   proves redundant.
4. Vectorization stays inside the current Array contracts, with no new
   language surface.

Every other rule above is the ratified default. Step 1 proposes the numeric
targets, which the owner ratifies before step 2 starts.
