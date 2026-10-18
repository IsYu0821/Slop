.text
.global syscall_entry
#ifdef __ELF__
.type syscall_entry,@function
#endif
syscall_entry:
    /* SYSCALL leaves user RIP in RCX and RFLAGS in R11; it does not switch RSP. */
    mov %rsp, SYSCALL_USER_RSP(%rip)
    mov SYSCALL_KERNEL_RSP(%rip), %rsp
    /* Linux guarantees every user register except RAX, RCX and R11 survives a syscall. */
    push %r11
    push %rcx
    push %rax
    push %rdi
    push %rsi
    push %rdx
    push %r8
    push %r9
    push %r10
    push %rbx
    push %rbp
    push %r12
    push %r13
    push %r14
    push %r15
    /* Frame: +0 r15 +8 r14 +16 r13 +24 r12 +32 rbp +40 rbx +48 r10 +56 r9
       +64 r8 +72 rdx +80 rsi +88 rdi +96 rax(nr) +104 rcx +112 r11.
       Linux ABI: rax=nr, rdi,rsi,rdx,r10,r8,r9=args. SysV call has arg7 on stack. */
    mov %rsp, SYSCALL_FRAME(%rip)
    mov 96(%rsp), %rdi
    mov 88(%rsp), %rsi
    mov 80(%rsp), %rdx
    mov 72(%rsp), %rcx
    mov 48(%rsp), %r8
    mov 64(%rsp), %r9
    push 56(%rsp)
    call syscall_dispatch_raw
    add $8, %rsp
    /* Park the dispatcher result in the saved-RAX slot so signal delivery and
       rt_sigreturn can restore the interrupted syscall return value. */
    mov %rax, 96(%rsp)
    cmpb $0, EXEC_PENDING(%rip)
    je .Lcheck_signal
    mov EXEC_RIP(%rip), %rdx
    mov %rdx, 104(%rsp)
    movb $0, EXEC_PENDING(%rip)
.Lcheck_signal:
    /* If a signal is pending, build a sigframe on the user stack and rewrite
       the register frame so SYSRET enters the user handler. The saved frame
       sits at an 8-mod-16 offset, so pad once to keep the SysV ABI stack
       16-byte aligned for the callee. */
    mov SYSCALL_FRAME(%rip), %rdi
    test %rdi, %rdi
    jz .Lcheck_sysret
    push %rax
    call signal_setup_frame
    pop %rax
    test %rax, %rax
    jz .Lcheck_sysret
.Lcheck_sysret:
    /* SYSRET requires canonical lower-half addresses. The checks run while the
       user register frame is still on the kernel stack so RDX stays untouched. */
    mov 104(%rsp), %rdx
    shr $47, %rdx
    jne .Lbad_pre
    mov SYSCALL_USER_RSP(%rip), %rdx
    shr $47, %rdx
    jne .Lbad_pre
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbp
    pop %rbx
    pop %r10
    pop %r9
    pop %r8
    pop %rdx
    pop %rsi
    pop %rdi
    /* The saved-RAX slot is now on top: rt_sigreturn restores it, a normal
       return delivers the dispatcher result parked there. */
    cmpb $0, SIGRET_PENDING(%rip)
    je .Ldrop_rax
    movb $0, SIGRET_PENDING(%rip)
    pop %rax
    jmp .Lfinish_sysret
.Ldrop_rax:
    mov (%rsp), %rax
    add $8, %rsp
.Lfinish_sysret:
    pop %rcx
    pop %r11
    mov SYSCALL_USER_RSP(%rip), %rsp
    sysretq
.Lbad_pre:
    mov 104(%rsp), %rcx
    mov SYSCALL_USER_RSP(%rip), %rsp
    jmp .Lbad
.Lbad:
    mov %rcx, %rdi
    mov %rsp, %rsi
    mov SYSCALL_KERNEL_RSP(%rip), %rsp
    and $-16, %rsp
    call bad_sysret
#ifdef __ELF__
.size syscall_entry,.-syscall_entry
#endif
