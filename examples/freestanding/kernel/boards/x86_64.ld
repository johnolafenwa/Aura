ENTRY(_start)
SECTIONS {
    . = 0x100000;
    image_start = .;
    .multiboot : { KEEP(*(.multiboot)) }
    .text : { KEEP(*(.text.boot)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    load_end = .;
    .bss (NOLOAD) : {
        *(.bss .bss.*) *(COMMON)
        . = ALIGN(16);
        . += 0x10000;
        stack_top = .;
    }
    bss_end = .;
    ASSERT(bss_end <= 0x200000, "kernel exceeds the initial identity mapping")
    /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}
