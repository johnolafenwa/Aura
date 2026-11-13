ENTRY(_start)
SECTIONS {
  . = 0x80000000;
  .text : { KEEP(*(.text.boot)) *(.text .text.*) }
  .rodata : { *(.rodata .rodata.*) *(.srodata .srodata.*) }
  .data : { *(.data .data.*) *(.sdata .sdata.*) }
  .bss : { *(.bss .bss.*) *(.sbss .sbss.*) *(COMMON) }
  . = ALIGN(16);
  scratch = .;
  . += 64;
  . += 0x10000;
  stack_top = .;
}
