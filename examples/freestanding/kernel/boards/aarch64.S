// Startup for QEMU's `virt` AArch64 machine, entered at EL1.
    .section .text.boot
    .globl _start
_start:
    ldr x0, =stack_top
    mov sp, x0
    mov x0, #(3 << 20)         // CPACR_EL1.FPEN: turn FP and NEON on
    msr cpacr_el1, x0
    isb
    ldr x0, =scratch
    bl kernel_main
    bl power_off

    // The memory after the stack is the heap's.
    .globl heap_start
heap_start:
    ldr x0, =heap_base
    ret

    .globl heap_size
heap_size:
    ldr x0, =0x100000
    ret
    .globl power_off
power_off:
    ldr x0, =0x84000008        // PSCI SYSTEM_OFF
    hvc #0
1:  wfe
    b 1b
