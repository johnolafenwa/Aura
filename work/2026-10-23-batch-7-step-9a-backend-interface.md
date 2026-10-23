# Batch 7 Step 9a: The Native Backend Interface

## Goal

Start step 9 of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md)
with part 9a of the step 9 plan: put one interface between the native
emitter and its code generator, with Cranelift as the only
implementation. Step 9b adds an LLVM implementation of the same interface,
and step 11 removes Cranelift. The emitter's translation and layout code
must reach Cranelift only through the interface, and objects must stay
byte-identical.

## Design

### Two traits

- `native_backend.rs` defines `ModuleBackend` and `FunctionBackend`.
  `ModuleBackend` declares functions and data, defines data, defines a
  function from a body closure, and writes the object. It has 5 methods.
- `FunctionBackend` builds one body. It has 76 methods: 1 for the module,
  8 for blocks, 3 for variables, 2 for stack slots, 5 for imports and
  addresses, 6 for calls and control flow, 7 for inspection and rewrites,
  and 44 instructions. A test-only method counts instructions.
- A call returns its instruction and its results together, so the emitter
  no longer asks for `inst_results`.
- `Val`, `Blk`, `Var`, `Slot`, `Inst`, `FuncHandle`, `DataHandle`,
  `FuncRef`, `DataRef`, and `SigRef` are `u32` handles. `ScalarType`,
  `IntCC`, `FloatCC`, `MemFlags`, `TrapCode`, `Linkage`, `Signature`, and
  `DataDescription` are the interface's own types. The `types` module keeps
  the short names, such as `types::I64`.
- A `Signature` names `CallConv::Aura`, the private convention that may
  return many values, or `CallConv::C`. Aura functions and contract
  `invoke` adapters use the first, and thunks, binders, callable adapters,
  `main`, and runtime imports the second. Cranelift lowers both to the
  host convention, as before.

### The emitter's peepholes

The retain and release peepholes read and rewrite instructions they built
earlier. The interface names those operations: `value_type`,
`integer_constant`, `only_calls_since`, and `replace_with_call`,
`replace_with_copy`, and `replace_with_nop`. An LLVM implementation can
answer them from its own instructions.

### Ownership of the module

- `NativeCodegen` holds the module backend in an `Option`.
  `define_body` takes it out, defines the function, and puts it back.
  The body closure gets the codegen state and the function backend, and
  reaches the module through `FunctionBackend::module` for the data and
  imports it declares while it runs.
- Each `define_*` method keeps its checks and calls `define_body`. Its old
  body moved, unchanged apart from the calls, into an `emit_*` method.
- `DefineError` keeps a body's own error apart from the code generator's,
  so only the second gets the `failed to define ...` prefix.

### Cranelift

- `native_backend_cranelift.rs` maps each handle to the Cranelift entity
  with the same index, so the emitter builds the same instructions in the
  same order.
- The Cranelift flags, the host ISA, `per_function_section`, and the
  implicit return area stay there. The flags test moved with them.
- `verify` checks the function with its text in the error, as
  `define_function` did before.

## Work Completed

- `native_backend.rs` and `native_backend_cranelift.rs` are new, with
  `native_backend_cranelift_tests.rs`.
- `native_codegen.rs`, `native_codegen_callables.rs`, and
  `native_codegen_tests.rs` name no Cranelift type. The emitter shrinks
  from 14,202 to 13,698 lines, and the callables file from 1,448 to
  1,339, mostly because a call no longer needs `.ins()` on its own line.
- The tests that built a function by hand use `with_scratch_function`,
  which builds a body that is never defined.
- The tests still parse objects with `cranelift_object::object`. That is
  the `object` crate, which step 11 can name directly.

## Verification

- Objects: all 629 run-pass fixture objects match `dd81b6d0` byte for
  byte, dumped with the step 8 harness before and after.
- Tests: `aura-compiler` lib passes 1,663 of 1,664.
  `package_io_helpers_cover_local_error_paths` fails because the
  container runs as root, which can write a read-only directory. The test
  covers package file writes, which this change does not touch.
- Lint: `cargo fmt --check`, the CI clippy command, and
  `scripts/check-hygiene.sh` pass.
- Coverage: lines 96.56%, functions 97.44%, and regions 95.48%, above
  the 96.46, 97.33, and 95.23 floors. `native_backend_cranelift.rs`
  covers 96.68% of its lines.

## Follow-Up

- Step 9b adds the inkwell implementation and the
  `AURA_NATIVE_CODEGEN=llvm` switch in `host_module_backend`.
