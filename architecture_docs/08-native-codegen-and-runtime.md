# Native Codegen And Runtime

This chapter explains Aurora's direct native backend and the runtime ABI it targets.

## What code generation is

Code generation is the stage that turns an intermediate representation into machine-level artifacts.

In Aurora, the direct backend:

- takes `MirModule`
- uses Cranelift to emit native code into an object file
- links that object file against Aurora's direct runtime

The code generator lives in [`native_codegen.rs`](../crates/aurora-compiler/src/native_codegen.rs). The runtime ABI it targets lives in [`native_runtime.rs`](../crates/aurora-compiler/src/native_runtime.rs).

## Aurora's two native build modes

From the CLI's perspective:

- `--backend direct`
  always uses the direct backend
- `--backend auto`
  tries direct first, then falls back to a MIR-runtime launcher binary if direct fails

That behavior is implemented in [`crates/aura/src/main.rs`](../crates/aura/src/main.rs).

## The direct backend pipeline

```mermaid
flowchart LR
    A["MirModule"] --> B["validate_module"]
    B --> C["classify DirectType"]
    C --> D["declare runtime imports"]
    D --> E["compile each MirFunction with Cranelift"]
    E --> F["emit host object bytes"]
    F --> G["link with native_runtime static library"]
```

## Aurora's type strategy for native codegen

Aurora does not lower every value the same way.

`native_codegen.rs` classifies values into:

- `DirectType::Scalar`
  simple scalar values such as integers, floats, bool, and unit
- `DirectType::PlainClass`
  classes that can be passed directly as flattened fields
- `DirectType::Opaque`
  rich values that are boxed and passed through runtime pointers

This is one of the most important design choices in Aurora's native backend.

### Why this matters

It lets Aurora avoid boxing everything while still supporting rich language features.

- simple data can stay cheap
- complex data can still reuse runtime semantics
- the code generator does not need bespoke low-level handling for every value form

## Runtime imports instead of inlined complexity

Aurora's native codegen does not implement every builtin directly in generated machine code.

Instead, it declares many runtime functions such as:

- string helpers
- vector/map/set helpers
- queue/task helpers
- file/network helpers
- enum and instance helpers
- boxing/unboxing helpers

That keeps code generation smaller and keeps runtime semantics centralized.

## The role of `native_runtime.rs`

The direct runtime is Aurora's native ABI layer. It is responsible for:

- hosting boxed `OpaqueValue` objects
- explicit refcount management
- numeric operation helpers
- string and collection helpers
- queue/task operations
- direct file and networking builtins
- runtime diagnostics with source context

Aurora uses explicit retain/release helpers because generated code may pass opaque pointers around independently of Rust's normal ownership system.

## `OpaqueValue` and refcounting

`native_runtime.rs` defines:

- `OpaqueValue`
  the heap box holding a `Value`
- `retain_ref_count`
- `release_ref_count`

The key idea is:

- generated native code can cheaply pass opaque values as pointer-sized handles
- the runtime preserves Aurora semantics by storing real `Value` data behind those handles

## A tiny direct-backend design example

This miniature example does not use Cranelift, but it shows the same architectural split: simple values are direct, complex values are boxed.

```rust
#[derive(Debug, Clone)]
enum Type {
    Int,
    Bool,
    Vec,
}

#[derive(Debug, Clone)]
enum DirectType {
    Scalar,
    Opaque,
}

fn classify(ty: &Type) -> DirectType {
    match ty {
        Type::Int | Type::Bool => DirectType::Scalar,
        Type::Vec => DirectType::Opaque,
    }
}

fn emit_add(left: DirectType, right: DirectType) -> Result<&'static str, String> {
    match (left, right) {
        (DirectType::Scalar, DirectType::Scalar) => Ok("emit native integer add"),
        _ => Err("route through runtime helper instead".to_string()),
    }
}
```

Aurora's real backend is more advanced, but the design lesson is the same:

- classify values
- emit fast paths for simple cases
- call runtime helpers for rich behavior

## Cranelift's role

Aurora uses Cranelift to:

- create function signatures
- build IR blocks and instructions
- manage variables and values
- emit object code for the host ISA

Aurora still does the language-specific work itself:

- mapping MIR semantics onto Cranelift operations
- deciding when to box or flatten
- selecting runtime helpers
- validating that the MIR shape is supported by the direct backend

## Validation before emission

Before codegen, Aurora validates the MIR module and functions. That protects the backend from malformed or unsupported input shapes.

This is good compiler engineering:

- reject impossible or unsafe IR early
- keep codegen assumptions explicit
- fail with diagnostics instead of generating nonsense

## How the direct runtime relates to the MIR runtime

The direct runtime and the MIR runtime are different execution engines, but they are designed to preserve the same language behavior.

They both depend on the same language-level concepts:

- `Value`
- class and enum semantics
- collection behavior
- task/queue behavior
- file and networking behavior
- numeric overflow and division diagnostics

## Files to study

- [`native_codegen.rs`](../crates/aurora-compiler/src/native_codegen.rs)
- [`native_runtime.rs`](../crates/aurora-compiler/src/native_runtime.rs)
- [`native_codegen_tests.rs`](../crates/aurora-compiler/src/native_codegen_tests.rs)
- [`native_runtime_tests.rs`](../crates/aurora-compiler/src/native_runtime_tests.rs)

## What comes next

Read [09-packages-and-module-loading.md](09-packages-and-module-loading.md) to see how Aurora finds modules, packages, workspaces, and dependencies before any of the execution paths run.
