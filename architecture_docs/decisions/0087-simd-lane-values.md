# ADR-0087: The `simd` module has lane values written in Aura

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0085 (target CPU), ADR-0086 (reordered reductions), the
  counted loops over scalar lists in the native backend

## Context

Native code vectorizes a counted loop over a list of numbers when LLVM is
allowed to. Two kinds of loop it is not allowed to touch:

- **A float sum.** `total += xs[i] * ys[i]` adds in source order, and no
  compiler may reorder float additions. The loop runs one element at a
  time at any register width.
- **A loop whose shape the vectorizer does not recognize**, such as one
  with a branch per element on a CPU without masked stores.

`Array.dot` and `Array.fast_sum` cover the common reductions with a fixed
order (ADR-0086). A program with its own kernel has no way to say "do
these four elements together".

Other languages answer with vector types whose operations map to machine
instructions: Rust's `std::simd`, C's intrinsics, Swift's `SIMD4`. Two
things in Aura shape the answer:

- **Results must not depend on the machine or the backend.** `aura run`
  on the interpreter and a native binary print the same output, and
  tests compare output exactly.
- **Integer arithmetic is checked.** Every `+` on an `int64` can report
  overflow.

## Decision

### The module

- **`simd` is a builtin module written in Aura**, in
  `crates/aura-compiler/std/simd.au`, like `iter` and `json`.
- **Four lane classes**: `F64x2`, `F64x4`, `F32x4`, and `F32x8`. Each is a
  public Copy class whose public fields `l0`, `l1`, and so on are the
  lanes.
- **Each class has** the associated functions `splat(value)` and
  `load(values, at)`, the methods `store(values, at)`, `min`, `max`,
  `abs`, `sqrt`, `mul_add`, `sum`, and `to_list`, and the operators `+`,
  `-`, `*`, `/`, and unary `-` between two values of the same class.
- **The operator traits `Sub`, `Mul`, `Div`, and `Neg`** are declared in
  `simd` beside the prelude's `Add`. The operators already dispatch to a
  trait by method name.

### Semantics

- **Every operation is the scalar operation in each lane.** The methods
  are ordinary Aura over the fields, so the interpreter runs them as
  written and there is one definition of every result.
- **`sum` adds in pairs**: `(l0 + l1) + (l2 + l3)`, and for eight lanes
  the two halves that way and then their sum.
- **`mul_add` is not fused.** It rounds the product and then the sum. A
  fused form would give different bits on a CPU without the instruction.
- **`min` and `max`** are `self if self < other else other` and the same
  with `>`. A NaN in `self` gives `other`.
- **Division by a zero lane traps with `AU4004`**, as scalar float
  division does.
- **`load` and `store` follow list indexing in each lane.** An index
  outside the list traps with `AU4003`, and a negative index counts from
  the end.

### Native code

- **Lane methods are copied into their callers.** The native backend
  marks every `simd` method that needs no frame for inlining, across
  codegen units. Lanes are unboxed floats in registers, and LLVM's SLP
  vectorizer joins the per-lane operations into vector instructions.
- **`abs`, `sqrt`, and float division no longer need a frame**, and
  `sqrt` is the LLVM intrinsic in place of a runtime call. This holds for
  scalar code too.
- **The register width follows `--target-cpu`.** The results do not.

### Loads and stores

- **A direct call of `load` or `store` on a local list with positional
  arguments lowers as the element accesses it stands for**:
  `F64x4.load(xs, i)` becomes `F64x4(l0=xs[i], l1=xs[i + 1], ...)`, and
  `v.store(xs, i)` becomes four element writes, with the index and the
  stored value evaluated once. This happens when the program is lowered
  to MIR, so both backends, the borrow checker, and the range proof of a
  counted loop see plain indexing. The loop then checks the range of
  every lane once, and a trap reports the call.
- **Every other call runs the method**: a list that is a field, named
  arguments, or `load` used as a function value. It is correct and checks
  each element.

### Stepped ranges

- **`for i in range(start, stop, step)` with a positive integer literal
  step counts on `int64` locals**, as a range without a step already
  did. A lane loop is written `for i in range(0, len(xs) - 3, 4)`, and it
  ran a thousand times slower through a `Range` value.
- **The count is exact at the limits of `int64`.** The loop tests
  `i < stop` once, before the first iteration. After the body it advances
  only while `i < last`, where `last` is `stop - step`, so the addition
  cannot overflow and the next value is below `stop`. When `stop` is too
  close to the least `int64` to subtract the step from, `last` is the
  start, and the loop runs once.
- **Each iteration after the first has that one test.** LLVM can then
  count the iterations, and a lane loop compiles to the lane instructions,
  an add, a compare, and a jump.
- **The range proof of a counted loop follows both ways into the body**:
  from the entry, where the index is the start, and from the advance,
  where it is below `last + step`. It keeps the strongest guard for each
  local, so a lane load of eight elements adds two conditions to the
  loop's entry and not sixteen.
- **A step that is not a positive literal, and a loop with `invariant`
  clauses, keep the `Range` value.**

## Alternatives rejected

- **Compiler-known vector types backed by LLVM vector values.** Each
  operation would need a second definition in the interpreter and in the
  verifier, and the two definitions could drift. Lanes as fields reach
  the same instructions through inlining and SLP.
- **Integer lanes.** Checked lanes need a compare in each lane and a
  branch for each operation, which costs what the lanes save. Wrapping
  integer operations exist as methods that currently box their operands.
  Integer lanes can follow when wrapping arithmetic is unboxed.
- **A fused `mul_add`.** Faster where the instruction exists, and a
  different answer where it does not.
- **Lane-wise comparisons and masks.** `min`, `max`, and `abs` cover the
  branch-free kernels measured so far. Masks need a lane type of their
  own.
- **A generic `Simd[T, N]`.** Aura has no integer type parameters.
- **Loading from `Array`.** An Array element read is a runtime call. The
  Array kernels are the vector path for Arrays.
- **A trap at the first lane's index when any lane is out of range.**
  The lane-by-lane rule is what the method's body says, and one rule for
  both the lowered and the called form keeps them the same program.

## Consequences

- A dot product over 1,024 `float64` elements with `F64x4` runs about
  three times as fast as the scalar loop. `benchmarks/simd` has six such
  kernels. On a baseline build each is at or ahead of the same lane struct
  in Rust. On a build for a CPU with 512-bit registers Rust is up to one
  and a half times as fast on two of them, because its loop vectorizer
  widens and unrolls the struct loop, and Aura keeps the width the
  program wrote.
- **Lane classes of 8 `float64` or 16 `float32` were measured and left
  out.** On the AVX-512 host they ran no faster than `F64x4` and `F32x8`:
  LLVM keeps 256-bit registers on that CPU, and what remains at 1,024
  elements is the cost of entering the function.
- Both backends print the same bits for every lane operation, and a
  build for a wider CPU changes speed only.
- A program that starts tasks polls for a yield in each loop iteration,
  so its lane loops are not unrolled across iterations. The arithmetic
  inside an iteration is still vector code.
- A trap inside a lane method that was called, such as a division by a
  zero lane, reports a line of `<std>/simd.au` and names the method and the
  caller in its call chain.
- User code can declare a class named `F64x4`. Only the classes of the
  builtin `simd` module get the lowered loads and stores.
