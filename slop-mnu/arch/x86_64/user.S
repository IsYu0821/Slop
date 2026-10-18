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

/* signal_deliver_asm: invoked from syscall.S before SYSRET when a signal is
   pending. Aligns the user stack, builds a minimal sigframe and enters the
   handler. The sigframe layout mirrors what sigreturn expects:
     +0  handler_rip   handler address
     +8  signal         signal number
     +16 saved_rsp      user stack before alignment
     +24 saved_rflags   rflags
     +32 saved_rcx      trampoline return address
     +40 handler_rsp    stack for handler
   sigreturn restores the frame and executes rt_sigreturn. */
.global signal_deliver_asm
#ifdef __ELF__
.type signal_deliver_asm,@function
#endif
signal_deliver_asm:
    /* Args: rdi = signal number, rsi = handler, rdx = rsp, rcx = rflags,
             r8 = saved_rcx (return address). */
    push %r8
    push %rcx
    push %rdx
    push %rsi
    push %rdi
    /* 16-byte align the stack for the handler. */
    mov %rsp, %rax
    and $-16, %rax
    sub $48, %rax
    mov %rax, %rsp
    /* Build the sigframe below the aligned stack. */
    mov %rdi, 40(%rax)        /* handler_rsp */
    mov %rcx, 32(%rax)        /* saved_rcx */
    mov %rdx, 24(%rax)        /* saved_rflags */
    mov %rsi, 16(%rax)        /* saved_rsp */
    mov %rdi, 8(%rax)         /* signal */
    mov %rdx, 0(%rax)         /* handler_rip (placeholder, overwritten below) */
    /* Set up handler entry: rdi = signal, stack = sigframe. */
    mov 0(%rax), %rdi
    mov %rax, %rsi
    jmp *%rdx
#ifdef __ELF__
.size signal_deliver_asm,.-signal_deliver_asm
#endif

/* signal_return_asm: invoked by the handler's return trampoline to execute
   rt_sigreturn. The sigframe address is passed in rdi. */
.global signal_return_asm
#ifdef __ELF__
.type signal_return_asm,@function
#endif
signal_return_asm:
    mov %rdi, %rsi
    mov $15, %rax              /* rt_sigreturn */
    syscall
#ifdef __ELF__
.size signal_return_asm,.-signal_return_asm
#endif
