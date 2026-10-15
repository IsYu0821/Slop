ENTRY(_start)
PHDRS {
  text PT_LOAD FILEHDR PHDRS FLAGS(5);
  rodata PT_LOAD FLAGS(4);
}
SECTIONS {
  . = 0x0000040000000000 + SIZEOF_HEADERS;
  .text : { *(.text .text.*) } :text
  . = ALIGN(4096);
  .rodata : { *(.rodata .rodata.*) } :rodata
  /DISCARD/ : { *(.comment*) *(.note*) *(.eh_frame*) }
}
