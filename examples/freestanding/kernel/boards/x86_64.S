// The Multiboot address header lets QEMU load a flat image at 1 MiB.
// It enters in 32-bit protected mode with paging disabled.
.section .multiboot,"a"
.balign 4
header:
    .long 0x1badb002, 0x10000, -(0x1badb002 + 0x10000)
    .long header, image_start, load_end, bss_end, _start

.section .text.boot,"ax"
.code32
.global _start
_start:
    cli
    cld
    lgdt gdt_pointer
    mov %cr4, %eax
    or $(1 << 5), %eax          // CR4.PAE
    mov %eax, %cr4
    mov $pml4, %eax
    mov %eax, %cr3
    mov $0xc0000080, %ecx       // IA32_EFER.LME
    rdmsr
    or $(1 << 8), %eax
    wrmsr
    mov %cr0, %eax
    or $(1 << 31), %eax         // CR0.PG
    mov %eax, %cr0
    ljmp $8, $long_mode

.code64
long_mode:
    mov $16, %eax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    lea stack_top(%rip), %rsp
    call kernel_main
    ud2

.section .rodata.boot,"a"
.balign 8
gdt:
    .quad 0, 0x00af9a000000ffff, 0x00cf92000000ffff
gdt_end:
gdt_pointer:
    .word gdt_end - gdt - 1
    .long gdt

// The first 2 MiB maps to itself with one large page. The linker checks
// that the image, including the interrupt stack, fits within it.
.section .data.boot,"aw"
.balign 4096
pml4:
    .quad pdpt + 3
    .zero 4088
pdpt:
    .quad pd + 3
    .zero 4088
pd:
    .quad 0x83
    .zero 4088
