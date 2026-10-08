ENTRY(_start)
PHDRS { interp PT_INTERP; dyn PT_DYNAMIC FLAGS(6); xseg PT_LOAD FLAGS(5); wseg PT_LOAD FLAGS(6); }
SECTIONS
{
  . = SIZEOF_HEADERS;
  .interp : { KEEP(*(.interp)) } :interp :xseg
  .hash : { *(.hash) } :xseg
  .gnu.hash : { *(.gnu.hash) } :xseg
  .dynsym : { *(.dynsym) } :xseg
  .dynstr : { *(.dynstr) } :xseg
  .rel.dyn : { *(.rel.dyn) *(.rel.iplt) } :xseg
  .rel.plt : { *(.rel.plt) } :xseg
  .text : { *(.text) *(.text.*) } :xseg
  .ARM.exidx : { *(.ARM.exidx*) } :xseg
  .ARM.extab : { *(.ARM.extab*) } :xseg
  .rodata : { *(.rodata) *(.rodata.*) *(.eh_frame) } :xseg
  .init : { KEEP(*(.init)) } :xseg
  .init_array : {
    __preinit_array_start = ABSOLUTE(.);
    KEEP(*(.preinit_array))
    __preinit_array_end = ABSOLUTE(.);
    __init_array_start = ABSOLUTE(.);
    KEEP(*(.init_array)) KEEP(*(.init_array.*))
    __init_array_end = ABSOLUTE(.);
  } :xseg
  .fini_array : {
    __fini_array_start = ABSOLUTE(.);
    KEEP(*(.fini_array)) KEEP(*(.fini_array.*))
    __fini_array_end = ABSOLUTE(.);
  } :xseg
  .fini : { KEEP(*(.fini)) } :xseg
  . = ALIGN(0x1000);
  .data : {
    *(.got) *(.got.plt)
    *(.data) *(.data.*) *(.sdata) *(.sdata.*)
    *(.bss) *(.bss.*) *(COMMON)
  } :wseg
  .dynamic : { *(.dynamic) } :dyn :wseg
  /DISCARD/ : { *(.note*) *(.comment) *(.ARM.attributes) *(.debug*) }
}
