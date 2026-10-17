.text
.global enter_user_asm
#ifdef __ELF__
.type enter_user_asm,@function
#endif
enter_user_asm:
    cli
    mov %r8, %rax
    mov %ax, %ds
    mov %ax, %es
    push %r8
    push %rsi
    push %rdx
    push %rcx
    push %rdi
    iretq
#ifdef __ELF__
.size enter_user_asm,.-enter_user_asm
#endif
