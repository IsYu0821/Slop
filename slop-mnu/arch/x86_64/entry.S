.section .text.boot,"ax"
.global _start
.type _start,@function
_start:
    cli
    lea boot_stack_top(%rip), %rsp
    xor %rbp, %rbp
    mov %rax, %rdi
    mov %rbx, %rsi
    call kmain
1:  hlt
    jmp 1b
.section .bss.stack,"aw",@nobits
.align 16
boot_stack:
    .skip 65536
boot_stack_top:
