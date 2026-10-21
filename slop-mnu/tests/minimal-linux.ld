ENTRY(_start)
PHDRS {
  text PT_LOAD FILEHDR PHDRS FLAGS(5);
  rodata PT_LOAD FLAGS(4);
  data PT_LOAD FLAGS(6);
}
SECTIONS {
  . = 0x0000000000400000 + SIZEOF_HEADERS;
  .text : { *(.text .text.*) *(.ltext .ltext.*) } :text
  . = ALIGN(4096);
  .rodata : { *(.rodata .rodata.*) } :rodata
  . = ALIGN(4096);
  .data : { *(.data .data.*) *(.ldata .ldata.*) } :data
  .bss : { *(.bss .bss.*) *(.lbss .lbss.*) *(COMMON) } :data
  /DISCARD/ : { *(.comment*) *(.note*) *(.eh_frame*) }
}
