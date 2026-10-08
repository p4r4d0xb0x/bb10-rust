ENTRY(_start)
PHDRS { seg PT_LOAD FLAGS(7); }
SECTIONS
{
  . = 0x10000;
  .text : { *(.text) *(.text.*) } :seg
  .ARM.exidx : { *(.ARM.exidx*) } :seg
  .ARM.extab : { *(.ARM.extab*) } :seg
  .rodata : { *(.rodata) *(.rodata.*) *(.eh_frame) *(.got) *(.got.plt) *(.sdata) *(.sdata.*) } :seg
  .data : {
    *(.data) *(.data.*)
    KEEP(*(.init)) KEEP(*(.init_array)) KEEP(*(.fini_array)) KEEP(*(.fini))
    *(.bss) *(.bss.*) *(COMMON)
  } :seg
  /DISCARD/ : { *(.note*) *(.comment) *(.ARM.attributes) *(.debug*) }
}
