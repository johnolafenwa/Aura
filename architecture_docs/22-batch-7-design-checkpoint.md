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

### Step 7 Measurements

Step 7 measured the 0.6.0 release candidate on the Linux container and on
the macOS arm64 baseline host, a Mac14,9. Each figure is the median of 11
alternating Aura and Rust runs, and every output matched.

| Program | Linux, Aura / Rust | macOS, Aura | macOS, Rust | macOS, Aura / Rust | Phase 1 target |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 5.8x | 42.65 ms | 2.95 ms | 14.5x | 8x |
| `int_list_sum` | 3.7x | 8.09 ms | 3.98 ms | 2.0x | 5x |
| `string_list_scan` | 5.3x | 13.05 ms | 3.65 ms | 3.6x | 10x |
| `dict_build_get` | 4.4x | 84.08 ms | 15.26 ms | 5.5x | 8x |
| `set_insert_contains` | 3.4x | 13.98 ms | 5.07 ms | 2.8x | 5x |
| `record_field_reads` | 11.0x | 67.02 ms | 5.01 ms | 13.4x | 8x |

`record_field_reads` misses on both hosts. `fib(30)` misses on macOS arm64,
where a second pass gave 14.1x. Phase 2 carries both gaps to the LLVM
targets.

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
| 4 | Inline list length and element reads from a fixed internal layout, with one bounds check. Done for `int64`, `float64`, and `bool` elements, plus unboxed scalar appends: `int_list_sum` went from 18.9x to about 10x Rust |
| 5 | Bounds-check removal in proven `for` loops, and direct `Array` element reads. Done for `for` loops over a list or a span, and for rank-1 `Array` reads by one `int64`: `int_list_sum` went from 106 to 79 million instructions, and indexed `float64` Array reads run about 34 times faster |
| 6 | Vectorized integer Array kernels with ordered first-trap reporting. Done: `int32` and `int64` operators, `wrapping_` and `saturating_` forms, and `sum()` run in 256-element blocks on machine integers. One-million-element `int64` addition went from about 37 ms to 0.9 ms |
| 6a | Unboxed `int64`, `float64`, and `bool` lists, added on 2026-10-20. Done: these lists store 8-byte or 1-byte scalars instead of 112-byte values, and native code reads them in place. `int_list_sum` went from about 9x to about 4.1x Rust |
| 6b | Fewer boxes in string, key, and membership code, added on 2026-10-21. Done: f-strings build in one string, literal `str` keys and `int64` set needles are not boxed, lent loop elements skip a second bounds check, and repeated type tags are dropped. `string_list_scan` went to about 5.3x Rust, `dict_build_get` to 4.4x, `set_insert_contains` to 3.4x, and `record_field_reads` to 11x |
| 7 | Re-measurement against the targets, then a preview release on Cranelift. Done: four of six programs meet their targets on both hosts, and Aura 0.6.0 shipped as `v0.6.0-preview` |

Steps 2 and 6 do not depend on the others. Step 4 depends on step 3 for the
retain and release it emits inline. Steps 3 and 5 work on MIR, so phase 2
keeps them.

### Phase 2: LLVM As The Only Native Backend

| Step | Work |
| --- | --- |
| 8 | Move the semantic decisions left in `native_codegen.rs` into checked MIR, so an emitter translates MIR and lays out values. [Step 8](#step-8-semantics-in-mir) lists the sub-steps |
| 9 | LLVM emitter through inkwell, pinned to the LLVM version rustc uses. CI and release builds provide libLLVM. The forced parity matrix, size gates, and per-function sections pass on LLVM |
| 10 | Runtime fast paths and inlining across the runtime boundary, with frame records kept |
| 11 | LLVM becomes the only native backend. Cranelift code and dependencies are removed, and the native cache format, size baselines, and docs are updated |
| 12 | Re-measurement against the targets, then a release |

During steps 9 and 10, both emitters build in CI so parity failures point at
the new emitter. Step 11 ends that.

Two facts need checking at step 9 before the emitter grows: whether inkwell
supports LLVM 22, and how much libLLVM adds to the `aura` binary.

### Step 8: Semantics In MIR

The direct emitter is 22,934 lines. About 7,200 choose a runtime helper by
receiver and member name, about 3,000 declare the helpers, about 2,600 track
ownership, and about 1,550 re-infer the types of MIR temporaries. Value
layout takes about 3,900. Layout stays in the backend, and the rest moves
into MIR, where the interpreter and every native backend read it.

| Sub-step | Work |
| --- | --- |
| 8a | Repeatable builds. Codegen iterated hash maps with a per-process seed, so 538 of 626 run-pass fixtures emitted different objects from one build to the next. Iteration that reaches the object is ordered, and a test compares two builds of a third of the fixtures |
| 8b | One runtime ABI table: each helper's symbol, parameters, and result, declared once for every emitter |
| 8c | Complete local types. MIR records the type of every temporary, and the validator checks them. The emitter's type inference goes away |
| 8d | Resolved runtime operations. Lowering resolves each built-in call to a typed MIR operation, with the specializations made so far. The interpreter dispatches on the same operation |
| 8e | Explicit ownership. A MIR pass inserts retain and release, pair cancellation runs on the result, and a check verifies every path releases what it owns. The interpreter treats them as no-ops |
| 8f | Explicit checks. Element reads that `IndexInRange` proves become unchecked MIR reads, and checked arithmetic names its trap |
| 8g | Measure what is left in the emitter and the compile-time cost, before step 9 |

The Cranelift emitter follows each sub-step until step 11 removes it, so the
parity matrix keeps comparing native code with the interpreter.

### Step 8 Results

Step 8g measured `ff35ee70`, the end of 8f, against `840f91ee`, the
`v0.6.0-preview` release that starts phase 2. Both ran on one shared
four-core Linux x86-64 container. Its Xeon CPU has no SHA extensions. Step 1
ran on a different host, so its build times do not compare with these. Raw
reports and scripts are in `work/2026-10-22-batch-7-step-8g-measurements/`.

#### Emitter Size

The emitter is `native_codegen.rs` with `native_codegen_callables.rs` and
`native_codegen_reachability.rs`, without tests. The MIR column counts the
`mir*.rs` files, the interpreter among them, with `runtime_ops.rs`,
`runtime_abi.rs`, and `union_layout.rs`.

| Commit | `native_codegen.rs` | Emitter | MIR and shared tables |
| --- | ---: | ---: | ---: |
| `840f91ee`, start of phase 2 | 22,934 | 24,480 | 40,345 |
| `4c6f1238`, start of 8c | 20,201 | 21,761 | 40,811 |
| `ff35ee70`, end of 8f | 14,942 | 16,524 | 49,082 |

Step 8 took 7,956 lines, 32%, out of the emitter and added 8,737 lines to
MIR and the shared tables.

#### What The Emitter Still Does

Each function or block of `native_codegen.rs` at `ff35ee70` is assigned to
the group it mostly serves. The callables file is layout and the
reachability file is translation.

| Group | Lines | Share | What it holds |
| --- | ---: | ---: | --- |
| Translation of MIR | 6,659 | 40% | setup, blocks, terminators, rvalues, arithmetic with the traps MIR names, runtime ops to helpers, prints, tasks, cleanup registration |
| Value layout, boxing, and ABI | 8,253 | 50% | scalar words, inline unions and callables, boxing and type tags, inline list and `Array` reads, loans as places, mutable sinks, thunks, FFI, frame descriptors, retains by layout |
| Semantic decisions | 1,612 | 10% | the items below |

The semantic decisions, with their lines in `native_codegen.rs`:

| Item | Lines | Where | Recommendation |
| --- | ---: | --- | --- |
| Method calls resolved by name. The emitter finds class methods, picks trait impls by specificity, dispatches a union receiver per member, and runs a candidate chain on an erased receiver. It also clones a union by the name `clone` and finds the `From` impl for `?` | 850 | 1980-2008, 6812-6854, 7111-7171, 9238-9392, 10134-10210, 10829-10865, 12097-12419, 12752-12956 | Move before step 9, about 550 MIR lines |
| Generic call type arguments, unified from parameter and argument types | 166 | 6139-6165, 14687-14825 | Move before step 9, about 80 MIR lines |
| Cleanup exit method named as a string, and `TaskGroup` closed inline | 147 | 1655-1700, 10741-10813, 13248-13275 | Move before step 9, about 60 MIR lines |
| Named arguments of runtime ops bound by name again | 144 | 577-721, 6216-6227 | Move before step 9, about 60 MIR lines |
| Argument defaults of `process` calls and `Supervisor.start` | 71 | 11168-11238 | Move before step 9, about 90 MIR lines |
| A scalar `add`, `sub`, `mul`, or `div` with an `Array` right operand picks `ArrayBinary` by member name | 48 | 7063-7110 | Move before step 9, about 40 MIR lines |
| Borrowed-element `len`, `get`, and comparison read a lent list element in place | 186 | 4869-4934, 6904-7023 | Fold into step 9. It picks a faster helper with the same result |

Step 8g checked the other known items and found them to be translation or
layout:

| Item | Group | Recommendation |
| --- | --- | --- |
| A cast picks its conversion helper from the source and target types, and the helper reports a value that does not fit | Translation, 4674-4793 | Fold into step 9. Give `Cast` a trap only if step 10 inlines conversions |
| An `int32` range check on every result and every store, because `int32` lives in a 64-bit word | Layout, 5181-5238 and 8671-8712 | Leave for LLVM. An `i32` layout gets the check from the trap MIR names |
| `no_call_since` drops a repeated type tag on one box | Layout, 8617-8641 | Leave for LLVM |
| The `transfer_*` helpers retain boxed Copy values and the emitter's own temporaries, which MIR does not track | Layout, 2660-3102 | Leave for LLVM, which needs the same ledger for its temporaries |
| `container_mutation_callables` matches mutating members by name | MIR validator, `mir.rs` 5517 | Key it on the op with the method work. It also matches a user method of the same name. Step 9 does not read it |

#### Compile Time

`aura build`, cold, medians of 11 alternating runs in seconds. The middle
column is everything between the front end and the link.

| Program | Build | `aura check` | Lowering, MIR, hash, and emit | Link |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 0.742 to 0.721 | 0.006 to 0.006 | 0.424 to 0.423 | 0.282 to 0.275 |
| Reference agent | 1.055 to 1.014 | 0.066 to 0.070 | 0.672 to 0.647 | 0.294 to 0.297 |
| `unix_tls_roundtrip` | 0.878 to 0.818 | 0.011 to 0.012 | 0.525 to 0.475 | 0.344 to 0.335 |

Callgrind splits the work of the `aura build` process, in millions of
instructions, base to head:

| Phase | Hello world | Reference agent | `unix_tls_roundtrip` |
| --- | ---: | ---: | ---: |
| SHA-256 of the 62 MB runtime archive | 3,341 to 3,344 | 3,341 to 3,344 | 3,341 to 3,344 |
| Front end | 6.6 to 6.6 | 252.5 to 252.5 | 33.7 to 33.7 |
| Lowering | 8.0 to 7.1 | 346.3 to 348.7 | 26.5 to 26.1 |
| `resolve_checks` | none to 0.4 | none to 2.4 | none to 0.8 |
| Ownership pass, with lending | none to 17.4 | none to 81.6 | none to 24.1 |
| Unreachable-function pruning | 114.2 to 2.0 | 202.7 to 42.9 | 123.2 to 12.4 |
| Boundary validators | 0.1 to 0.4 | 135.2 to 130.9 | 19.0 to 18.6 |
| Emission, Cranelift included | 5.2 to 5.6 | 970.7 to 797.1 | 374.1 to 169.3 |
| Compiler work without the hash | 135.8 to 41.1 | 1,923.1 to 1,672.6 | 579.5 to 287.8 |

- **Step 8 made builds no slower.** The passes it added cost 18 to 84
  million instructions, about 5% of the compiler's work on the reference
  agent. Faster pruning and less emitted code more than pay for them.
- **The ownership pass is the top new cost.** Allocation takes about 37%
  of its instructions on the reference agent. Recursive type walks with no
  memo, `lending::may_reach_instance` first, rebuild substituted types for
  every call. The pass also runs before pruning. Hello world owns 111
  functions and emits one, and the reference agent owns 228 and emits 106.
- **The archive hash dominates the build.** `aura build` hashes the whole
  runtime archive for the native cache identity. On this host that is 98%
  of a hello world build process and two thirds of a reference agent's,
  about 0.35 s. Hello world builds in 0.72 s here, over the phase 2 target
  of 0.6 s, before LLVM adds anything. Step 8 did not change it.

#### Runtime

Medians of 31 alternating runs in milliseconds. `fib(30)` and 10,000 tasks
time GO to DONE. The rest time the whole process.

| Program | `840f91ee` | `ff35ee70` | Rust | Aura / Rust, before and after | Phase 2 target |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 40.14 | 39.09 | 4.13 | 9.7x, 9.5x | 3x |
| 10,000 tasks | 744.1 | 742.7 | 7.89 | 94.3x, 94.1x | tracked |
| `int_list_sum` | 10.17 | 10.88 | 3.73 | 2.7x, 2.9x | 2x |
| `string_list_scan` | 16.12 | 16.76 | 3.65 | 4.4x, 4.6x | 4x |
| `dict_build_get` | 116.67 | 116.72 | 23.73 | 4.9x, 4.9x | 4x |
| `set_insert_contains` | 21.20 | 21.52 | 5.97 | 3.6x, 3.6x | 3x |
| `record_field_reads` | 106.74 | 104.59 | 9.31 | 11.5x, 11.2x | 4x |

Runtime did not change. Callgrind counts the same work in both builds:
`int_list_sum` runs 73.21 and 73.12 million instructions, `string_list_scan`
150.97 and 150.89 million, and `record_field_reads` 692.55 and 693.17
million. So the 7% wall-clock gap on `int_list_sum` is code layout, as 8e
found for `record_field_reads`. Binary sizes match, except
`record_field_reads`, which is 4 KiB smaller. TCP fan-out has no Rust
binary on this host and was not run. This host's ratios run higher than
the step 7 Linux ones, for both commits.

#### What Step 9 Inherits

- MIR with a type for every local, a resolved op for every built-in call,
  explicit `Retain`, `Release`, `Lend`, and `EndLend`, a named trap on
  every numeric operation, and unchecked reads where a fact proves them.
- One runtime ABI table for the 386 helpers.
- About 8,250 lines of layout to rebuild in LLVM terms, and 6,650 lines
  of translation to rewrite against inkwell.
- The 1,612 lines of semantic decisions above, unless they move first.
- The two checks the plan already names: inkwell's LLVM 22 support, and
  the size libLLVM adds to `aura`.

#### Recommendation

Add one sub-step, 8h, before step 9. It moves every semantic item above
except borrowed-element fusion, in two PRs:

1. **Bound runtime op arguments.** Lowering binds named arguments to op
   parameters and fills the `process` and `Supervisor.start` defaults. A
   resolved op covers scalar-left `Array` arithmetic. Each generic call
   records its type arguments. `container_mutation_callables` keys on
   ops. About 300 MIR lines, and the emitter loses about 400.
2. **Resolved method calls.** A member call names the function it calls.
   A union or erased receiver carries its candidates in specificity order.
   `?` names its `From` function, and each cleanup action names its
   callee. About 550 MIR lines, and the emitter loses about 650.

These decisions choose which user code or which default runs. The emitter
re-derives them from names with sema's helpers, and the interpreter does
too. A second emitter would make a fourth copy of each, about 1,400 lines
of inkwell code that the parity matrix would have to keep in line. The
passes so far cost about 5% of compiler work, so the move is cheap at
build time. After 8h, start step 9.

Two build-time fixes help the phase 2 target and do not block step 9:
memoize the ownership pass's type walks and run it after pruning, and
stop hashing the full runtime archive on every `aura build`.

### Step 9 Results

Step 9c measured LLVM on the six phase 2 targets with `default<O1>`,
`default<O2>`, and `default<O3>`, against Cranelift and Rust. The tables and
method are in
[the 9c note](../work/2026-10-23-batch-7-step-9c-llvm-tuning.md). The
container's wall times are noisy, so this table uses callgrind
instructions for one whole run, in millions:

| Program | Cranelift | LLVM `O3` | Rust | LLVM / Rust | Phase 2 target |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 523.0 | 483.9 | 39.5 | 12.3x | 3x |
| `int_list_sum` | 73.1 | 42.2 | 13.2 | 3.2x | 2x |
| `string_list_scan` | 150.8 | 137.5 | 13.1 | 10.5x | 4x |
| `dict_build_get` | 942.0 | 936.9 | 179.0 | 5.2x | 4x |
| `set_insert_contains` | 148.3 | 144.5 | 31.6 | 4.6x | 3x |
| `record_field_reads` | 693.1 | 690.5 | 36.0 | 19.2x | 4x |

- The ratios count instructions, not time, so they do not compare with the
  wall-clock targets directly. They show where the work is.
- LLVM alone moves little. Generated code was never most of the time:
  the runtime calls are, and step 10 inlines across that boundary.
- `O1`, `O2`, and `O3` build the same executable for four targets. `O3`
  runs `int_list_sum` in 22% fewer instructions, by moving the list's
  inline layout check out of the loop, so `O3` is the default.
- A cold reference agent build takes 1.27 s on LLVM and 1.05 s on
  Cranelift here, inside the 1.5 s target. Hello world takes about 0.7 s
  on both, over the 0.6 s target, because a cold build still hashes the
  runtime archive.
- Every native binaries subject passes the size gate on LLVM, with the
  Cranelift baselines. The reference agent links 2% smaller.

### Step 10 Progress

Step 10a records frames inline. Generated code pushes and pops its frame
descriptor on the task's frame stack, and calls the runtime only for the
cold cases. The method and tables are in
[the 10a note](../work/2026-10-23-batch-7-step-10a-frame-fast-path.md).
Callgrind instructions for one whole run, in millions:

| Program | Cranelift before | Cranelift after | LLVM before | LLVM after | LLVM / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 523.0 | 210.6 | 483.9 | 115.0 | 2.9x |
| `int_list_sum` | 73.1 | 73.1 | 42.2 | 42.2 | 3.2x |
| `string_list_scan` | 150.8 | 150.9 | 137.5 | 137.6 | 10.5x |
| `dict_build_get` | 942.0 | 940.4 | 936.9 | 935.3 | 5.2x |
| `set_insert_contains` | 148.3 | 148.9 | 144.5 | 144.7 | 4.6x |
| `record_field_reads` | 693.1 | 693.2 | 690.5 | 690.5 | 19.2x |

- `fib(30)` on LLVM is inside its 3x target by instruction count. The
  other five barely call Aura functions, so 10a leaves them alone. The
  later parts of step 10 target their runtime calls.
- Traces, task ancestry, and the depth limit of 256 calls are unchanged.

Step 10b replaces each task's map of owned values with a list linked
through the value headers, and retains and releases values in place. A
per-thread cache reuses freed value boxes. The method and tables are in
[the 10b note](../work/2026-10-23-batch-7-step-10b-ownership-ledger.md).
Callgrind instructions, in millions:

| Program | Cranelift before | Cranelift after | LLVM before | LLVM after | LLVM / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 210.6 | 210.6 | 115.0 | 115.0 | 2.9x |
| `int_list_sum` | 73.1 | 73.1 | 42.2 | 42.2 | 3.2x |
| `string_list_scan` | 150.9 | 145.1 | 137.5 | 130.9 | 10.0x |
| `dict_build_get` | 940.4 | 785.9 | 935.3 | 764.8 | 4.3x |
| `set_insert_contains` | 148.9 | 141.9 | 144.6 | 136.5 | 4.3x |
| `record_field_reads` | 693.2 | 588.3 | 690.5 | 570.6 | 15.8x |

- `dict_build_get` drops 18% and `record_field_reads` 17% on LLVM. The
  plan asked for 25%. The ledger and its lookups cost about 15% of each
  run before 10b, so removing them all could not reach 25%. The box cache
  adds most of the rest.
- What remains is mostly allocation, cloning, hashing, and type tags,
  which 10d and 10e target.
- A task that fails or is cancelled still frees every value it held.

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

On 2026-10-21 the owner chose to move the emitter's semantic decisions into
MIR rather than into a separate backend IR. That decision is
[Step 8](#step-8-semantics-in-mir).

Every other rule above is the ratified default. On 2026-10-20 the owner also
approved every numeric target that step 1 proposed, listed in
[Targets](#targets).
