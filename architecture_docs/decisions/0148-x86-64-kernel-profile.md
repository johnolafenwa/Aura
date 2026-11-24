# ADR-0148: x86-64 Kernel Code Generation

- Status: Accepted
- Date: 2026-11-24
- Design checkpoint: [Freestanding Machine Code](../37-systems-machine-design-checkpoint.md)

## Context

An interrupt entry can run before startup enables SSE. Kernel code may
also interrupt a thread whose vector registers hold live values. The
default x86-64 freestanding target uses SSE2 and the System V red zone,
the 128 bytes below the stack pointer that a leaf may use without
allocating stack space. An interrupt using that stack can overwrite it.

## Decision

Use `--freestanding --target x86_64-unknown-none --kernel` with
`aura build` or `aura check`. Omit the target on an x86-64 host.
The ordinary freestanding build keeps its position-independent code,
default code model, SSE2 floating-point ABI and red zone.

Kernel mode selects LLVM's static relocation model and kernel code
model. The linker can place code and data in the high two GiB. The
backend disables MMX, SSE, SSE2 and x87 on the x86-64 baseline CPU,
enables software floating point, and attaches the no-red-zone attribute
to every function before emitting the object. This covers fallback memory
routines and functions the optimizer creates.

## Linking

Compile foreign code, interrupt handlers and support routines with matching
register, stack and address conventions. Floats pass as IEEE bit patterns
in integer registers. When an operation needs software arithmetic, the
kernel supplies the LLVM compiler-runtime routine, such as `__adddf3`.
A square root can require `sqrtf` or `sqrt` using the same ABI. Inspect
undefined symbols to find the routines one object needs. Integer-only
programs need no floating-point library.

## Evidence

Object tests inspect the relocation for a trap's source address and
disassemble a float addition and a register-spilling leaf function. The
leaf's volatile accesses force stack spills in both profiles. Ordinary
freestanding code can spill below the stack pointer. Kernel code allocates
space first.
