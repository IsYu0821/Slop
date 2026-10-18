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

.global resume_user_asm
#ifdef __ELF__
.type resume_user_asm,@function
#endif
/* resume_user_asm(frame, user CS, user SS): restores a UserFrame snapshot and
   iretq's back to CPL3 with RAX set by the frame (0 for fork children). */
resume_user_asm:
    push %rdx
    push 120(%rdi)
    mov 112(%rdi), %rax
    push %rax
    push %rsi
    push 104(%rdi)
    mov %rdx, %rcx
    mov %cx, %ds
    mov %cx, %es
    mov 0(%rdi), %r15
    mov 8(%rdi), %r14
    mov 16(%rdi), %r13
    mov 24(%rdi), %r12
    mov 32(%rdi), %rbp
    mov 40(%rdi), %rbx
    mov 48(%rdi), %r10
    mov 56(%rdi), %r9
    mov 64(%rdi), %r8
    mov 72(%rdi), %rdx
    mov 80(%rdi), %rsi
    mov 96(%rdi), %rax
    mov 88(%rdi), %rdi
    iretq
#ifdef __ELF__
.size resume_user_asm,.-resume_user_asm
#endif

