/* Minimal exec target for the A1 stress battery: no env/ppid assumptions,
   no output, exit(0). Raw Linux x86-64 syscalls only, so the same file
   runs on native Linux (reference) and in the guest. */
.global _start
.section .text
_start:
    mov $60,%rax
    xor %edi,%edi
    syscall
1:  jmp 1b
