/* AP trampoline. Entered in real mode by SIPI, transitions 16 -> 32 ->
   64 bit, then jumps to the kernel AP entry.

   The BSP always copies this blob to TRAMPOLINE_ADDR (0x7000) and the
   SIPI vector names that page (0x07), so every address below is an
   absolute low-memory constant: no link-time symbol may appear as a
   runtime value (only relative jumps). Layout of the 4 KiB page:
     0x7000: 16-bit entry
     0x7100: 32-bit transition
     0x7200: GDT (null, 32-bit code, 32-bit data, 64-bit code,
                  64-bit data)
     0x7240: GDT pseudo-descriptor (limit u16, 32-bit base)
     0x7300: mailbox written by the BSP (5 x u64, see below)
     0x7400: 64-bit stub (runs identity-mapped, jumps high)
   Mailbox (written by the BSP before SIPI):
     0x7300: kernel PML4 physical address (for CR3)
     0x7308: 64-bit AP entry virtual address (absolute high-half)
     0x7310: LAPIC MMIO virtual base (direct map)
     0x7318: APIC-ID table virtual address ([u32; 64])
     0x7320: per-CPU stack-top table virtual address ([u64; 64])
   Real-mode stack: SS=0, SP=0x7e00 (inside this page). */

#define TRAMP_BASE 0x7000
#define TRAMP_GDT 0x7200
#define TRAMP_GDTPTR 0x7240
#define TRAMP64_OFF 0x7400

#define MB_PML4 0x7300
#define MB_ENTRY 0x7308
#define MB_LAPIC 0x7310
#define MB_IDS 0x7318
#define MB_STACKS 0x7320

#define IA32_EFER 0xc0000080
#define EFER_LME (1<<8)
#define EFER_NXE (1<<11)

#define CR4_PAE (1<<5)
#define CR4_OSFXSR (1<<9)
#define CR0_PE 1
#define CR0_WP (1<<16)
#define CR0_PG (1<<31)

.code16
.global trampoline_start
trampoline_start:
    cli
    xor %ax, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    mov $0x7e00, %sp
    /* Stage marker: real mode entered. */
    .byte 0x66
    movl $0x11111111, 0x7328
    /* Load our GDT (32-bit base form for the flat descriptors). */
    .byte 0x66
    lgdt TRAMP_GDTPTR
    /* Enter protected mode. */
    mov %cr0, %eax
    or $CR0_PE, %eax
    mov %eax, %cr0
    ljmp $0x08, $0x7100

.org 0x100
.code32
prot32:
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov $0x7e00, %esp
    /* Stage marker: protected mode with our segments. */
    movl $0x22222222, 0x7328
    /* PAE + OSFXSR on (matches the BSP); keep the rest as found. */
    mov %cr4, %eax
    or $(CR4_PAE|CR4_OSFXSR), %eax
    mov %eax, %cr4
    /* Point at the kernel page tables. */
    mov MB_PML4, %eax
    mov %eax, %cr3
    /* Long mode + no-execute enable. */
    mov $IA32_EFER, %ecx
    rdmsr
    or $(EFER_LME|EFER_NXE), %eax
    wrmsr
    /* Paging (and supervisor write protection, like the BSP). */
    mov %cr0, %eax
    or $(CR0_PG|CR0_WP), %eax
    mov %eax, %cr0
    /* Far jump into 64-bit mode at our identity address. */
    ljmp $0x18, $0x7400

/* GDT: null, 32-bit code, 32-bit data, 64-bit code, 64-bit data. */
.org 0x200
tramp_gdt:
    .quad 0x0000000000000000
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00af9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x0000000000000000
    .quad 0x0000000000000000
.org 0x240
tramp_gdtptr:
    .word 0x37
    .long 0x7200
.org 0x300
tramp_mailbox:
    .space 0x40, 0

.code64
.org 0x400
tramp64:
    /* We run identity-mapped with the old real-mode stack. Find our CPU
       by reading the Local APIC ID through the direct map. */
    mov $0x7300, %rbx
    /* Stage marker: 64-bit mode on our stack. */
    movl $0x33333333, 0x28(%rbx)
    mov 0x10(%rbx), %rax
    mov 0x20(%rax), %eax
    shr $24, %eax
    /* Linear search of the BSP-written APIC-ID table (at most 64). */
    mov 0x18(%rbx), %rcx
    xor %edx, %edx
1:
    cmp $64, %edx
    jae 3f
    mov (%rcx,%rdx,4), %esi
    cmp %eax, %esi
    je 2f
    inc %edx
    jmp 1b
2:
    /* Stage marker: APIC ID matched a table entry. */
    movl $0x44444444, 0x28(%rbx)
    /* Load this CPU's kernel stack (tops table indexed by CPU). */
    mov 0x20(%rbx), %rcx
    mov (%rcx,%rdx,8), %rsp
    /* Hand RDI=APIC ID, RSI=cpu index to the kernel entry. */
    mov %eax, %edi
    mov %edx, %esi
    mov 0x08(%rbx), %rax
    jmp *%rax
3:
    cli
4:
    hlt
    jmp 4b
.global trampoline_end
trampoline_end:
