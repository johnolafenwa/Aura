# Freestanding Machine Code

## Goal

Build kernel entry points, assembly and privileged hardware operations for
the three 64-bit freestanding targets. Exercise the x86-64 interrupt ABI
on a booted machine. The design is recorded in
[Freestanding Machine Code](../architecture_docs/37-systems-machine-design-checkpoint.md).

## Evidence

The original RISC-V and AArch64 examples both booted in QEMU. Each printed
`p=168`, `c=111`, `r=1000`, `n=100`, then
`trap 1 main.au:154:13` and powered off.

On x86-64, disassembly of the original freestanding build showed leaf
spills 48 bytes below the stack pointer. The same volatile-memory fixture
built with `--kernel` reserved space before spilling. The soft-float ABI
test found a call to `__adddf3` and exercised the integer-register
argument and return convention.

The initial verifier regression reported an unknown function at the
`asm` statement. The test also checks the named proof diagnostic and
compiles the same source with proved-check removal enabled.

The x86-64 interrupt board printed `breakpoint`, `timer` and `ok`
and exited QEMU with status 33. Both virt boards exited with status 0.
The interrupt table and two event bytes use C-backed storage.

The systems-data merge supplied mutable module storage. Its section and
alignment attributes apply to the actual writable object cell. The object
test checks initialized data, alignment and a single definition.
