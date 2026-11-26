# Startup for QEMU's `virt` RISC-V machine, entered in machine mode.
    .section .text.boot
    .globl _start
_start:
    la sp, stack_top
    li t0, 0x6000              # mstatus.FS: turn the float registers on
    csrs mstatus, t0
    la a0, scratch
    call kernel_main
    call power_off

    # The memory after the stack is the heap's.
    .globl heap_start
heap_start:
    la a0, heap_base
    ret

    .globl heap_size
heap_size:
    li a0, 0x100000
    ret
    .globl power_off
power_off:
    li t0, 0x100000            # the SiFive test device ends the emulation
    li t1, 0x5555
    sw t1, 0(t0)
1:  wfi
    j 1b
