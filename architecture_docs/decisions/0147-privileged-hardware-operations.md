# ADR-0147. Privileged hardware operations in mem

- Status: Accepted
- Date: 2026-11-24
- Design checkpoint: [Freestanding machine code](../37-systems-machine-design-checkpoint.md)

## Decision

Each freestanding target adds typed functions to the existing `mem` module.
The functions are ordinary Aura definitions with side-effecting assembly
bodies. Selecting the source by architecture keeps the instructions and
their operand types together. Qualified calls and imported aliases resolve
through the same declarations. The existing package permission
`allow_ffi = true` covers these imports.

Every target provides `enable_interrupts()`, `disable_interrupts()` and
`halt()`, each returning `None`.

| Architecture | Interrupt bit | Wait instruction |
| --- | --- | --- |
| x86-64 | IF in RFLAGS | HLT |
| AArch64 | I in DAIF (IRQ) | WFI |
| RISC-V | MIE in mstatus (machine-mode interrupts) | WFI |

A wakeup can resume at the instruction following `halt`. The processor
and its interrupt controller determine which interrupts arrive.

On x86-64, `in_uintN(port: uint16) -> uintN` and
`out_uintN(port: uint16, value: uintN) -> None` use DX and AL, AX or EAX
for widths 8, 16 and 32. `read_msr(index: uint32) -> uint64` and
`write_msr(index: uint32, value: uint64) -> None` transfer the full
EDX:EAX value of the model-specific register selected by ECX.

## Named registers

`read_<name>() -> uint64` reads a system register.
`write_<name>(value: uint64) -> None` writes one.

| Architecture | Read access | Read and write access |
| --- | --- | --- |
| x86-64 | | cr0, cr2, cr3, cr4, cr8 |
| AArch64 | midr_el1, mpidr_el1, currentel, cntpct_el0, cntfrq_el0 | sctlr_el1, tcr_el1, ttbr0_el1, ttbr1_el1, mair_el1, vbar_el1, esr_el1, far_el1, elr_el1, spsr_el1, cntp_ctl_el0, cntp_cval_el0 |
| RISC-V | mhartid, misa, cycle, time | mstatus, mie, mip, mtvec, mscratch, mepc, mcause, mtval, medeleg, mideleg, sstatus, sie, sip, stvec, sscratch, sepc, scause, stval, satp |

Each accessor performs the named access. The caller supplies the required
privilege, supported register bits, and any architectural synchronization
that the surrounding sequence requires. Verification treats the assembly
bodies as opaque. The checker reports AU2097 when a program requests a
machine operation outside the profiles that provide it.
