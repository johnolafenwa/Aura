# ADR-0149 PC Interrupts Under QEMU

Accepted 2026-11-24. [Freestanding Machine Code](../37-systems-machine-design-checkpoint.md)

## Decision

Boot an x86-64 PC alongside the two virt boards. A flat Multiboot image
starts in protected mode at 1 MiB. An assembly stub loads the GDT, maps
the first 2 MiB with a large page, enables long mode and sets the stack.
The linker checks that the image and stack fit in the mapping.

Compile the Aura program with the x86-64 kernel profile. Use an interrupt
table constructed in Aura and the real 8259 controller and 8254 timer
emulated by QEMU. A software breakpoint and a hardware timer interrupt
exercise the interrupt return independently. C supplies addressable
storage and symbol addresses. Aura encodes the gates, loads the IDTR,
configures the devices, handles the interrupts and writes to COM1.

## Evidence

The runner requires QEMU's debug-exit status 33. The kernel requests it
only after returning from both handlers and checking their event bytes.
The host compares the serial transcript and bounds the boot with a
timeout. These checks distinguish a successful return from a trap, an
early reset, a hung kernel or a guest that only prints the expected text.

The Linux x86-64 native-binaries job uses its release compiler to boot
all three machines. The AArch64 and RISC-V runs also check their serial
transcripts and board-specific shutdown.
