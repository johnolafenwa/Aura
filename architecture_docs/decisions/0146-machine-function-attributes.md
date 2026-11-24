# ADR-0146 Machine Entries And Object Placement

- Status: Accepted
- Date: 2026-11-24
- Design checkpoint: [Freestanding Machine Code](../37-systems-machine-design-checkpoint.md)

## Decision

Freestanding declarations accept attributes under `@machine`. An ordinary
decorator still computes a function at runtime. A machine attribute describes
a declaration to the checker and object writer.

`section` sets the ELF section name. `align` takes a power of two in bytes
through 2^29. Both also apply to module data. A literal module constant
with placement has storage of its declared scalar width. The backend uses
the same placement information for writable and read-only data.
`link_name` sets a function's symbol and preserves the source name for Aura
calls. `used` roots a function in the freestanding reachability walk and
places it in LLVM's retained symbol list. Its section survives ELF linker
garbage collection. `noinline` and `always_inline` control optimization.

## Entry Conventions

- `naked` replaces the prologue, epilogue and entire body with one
  operand-free assembly template. Its author owns the ABI and return.
- `interrupt("x86_64")` enters on an x86-64 interrupt stack.
  `interrupt("x86_64_error")` accepts the extra hardware error code as
  its one uint64 parameter. The wrapper saves the caller-saved registers,
  clears the direction flag, aligns the stack for the SysV call, restores
  registers, and removes the error word where applicable before IRETQ.
  The default x86 object saves x87 and XMM state with FXSAVE64. A kernel
  build omits floating state instructions. Compile the interrupted code
  without a red zone and compile its complete handler call graph for the
  same register contract, including foreign callees.
- `interrupt("aarch64")` handles an EL1 exception with an initialized,
  aligned stack and enabled FP/NEON. It preserves caller-saved integer
  registers, LR, all vector registers, FPCR, FPSR, ELR_EL1 and SPSR_EL1.
  ERET restores the interrupted PC and flags.
- `interrupt("riscv64")` handles a machine-mode exception with a valid,
  aligned LP64D stack. It preserves caller-saved integer registers, mepc,
  mstatus, all live FP registers and fcsr. When FS was off, it enables
  it for the ordinary call and restores its original state before MRET.

The ordinary target ABI protects the remaining callee-saved registers
across the call to the Aura body. Startup owns vector selection, stack
provisioning and the rules for interrupt nesting. These conventions cover
the project's baseline instruction sets. The checker rejects a mismatched
target, calling an interrupt entry as a function, and a signature outside
the hardware convention.
