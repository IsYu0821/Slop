/* The layout of the vector system call entry point.
 *
 * The file offset and the address of every loadable segment are the same,
 * so the C library, which computes the load address of the page from the
 * last loadable segment, finds the entry points at the addresses the
 * kernel maps them to.
 *
 * The first page holds the program headers and the tables that describe
 * the entry points, the second page holds the code.
 */
ENTRY(__vdso_clock_gettime)

SECTIONS
{
	. = 0x1000;
	.text : { *(.text*) }
	. = 0x2000;
	.dynsym : { *(.dynsym) }
	.hash : { *(.hash) }
	.gnu.hash : { *(.gnu.hash) }
	.dynstr : { *(.dynstr) }
	.dynamic : { *(.dynamic) }
	/DISCARD/ : { *(.note*) *(.comment) *(.symtab) *(.strtab) }
}
