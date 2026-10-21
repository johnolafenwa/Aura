# Batch 7 Step 8b: One Runtime ABI Table

## Goal

Declare each direct runtime helper once, in a form every emitter reads.
This is step 8b of the [Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
The LLVM emitter in step 9 declares the same helpers from the same table.

## Work Completed

- `crates/aura-compiler/src/runtime_abi.rs` lists all 386 helpers. Each
  `RuntimeFn` variant names its symbol, its parameter words, and its result
  word as an `AbiType` (`I32`, `I64`, or `F64`).
- `RuntimeTable<T>` holds one value per helper and is indexed by
  `RuntimeFn`. The emitter keeps one table of `FuncId` for the object and
  one table of `FuncRef` for each function it compiles.
- The direct emitter used to name every helper four times: a `FuncId`
  field, a declaration, a per-function `declare_func_in_func` call, and a
  `FuncRef` field. Those are gone. `native_codegen.rs` went from 22,944 to
  20,201 lines.
- Tests check that every helper has a unique `aura_direct_` symbol and that
  a table returns each helper's own entry.

## Verification

- All 626 run-pass fixtures emit byte-identical objects before and after the
  change.
- Clippy, the compiler and CLI suites, and the backend parity matrix pass.

## Follow-Up

- Step 8c records the type of every MIR temporary.
