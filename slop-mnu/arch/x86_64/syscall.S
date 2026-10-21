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
    movb $0, SIGRET_VALID(%rip)
    call syscall_dispatch_raw
    add $8, %rsp
    /* The dispatcher result travels back to the user in RAX. The saved RAX
       slot keeps the syscall number, so an interrupted syscall can be
       restarted with the same number. The result is parked in a scratch
       cell because the signal check below is a call. */
    mov %rax, SYSCALL_RESULT(%rip)
    cmpb $0, EXEC_PENDING(%rip)
    je .Lcheck_signal
    /* An execve redirects the return to the new image. */
    mov EXEC_RIP(%rip), %rdx
    mov %rdx, 104(%rsp)
    movb $0, EXEC_PENDING(%rip)
.Lcheck_signal:
    /* A queued signal is delivered before the process returns to user mode.
       The kernel builds the published rt_sigframe on the user stack and
       rewrites the register frame so the handler is entered with the signal
       number, the siginfo pointer and the ucontext pointer. The frame lives
       on this task's kernel stack, so a task that blocked inside a syscall
       still finds its own context. */
    mov %rsp, %rdi
    /* The saved frame holds an odd number of words, so the stack is 8 mod 16
       here. The System V ABI wants 16-byte alignment at a call, so the
       alignment is restored before the call. */
    sub $8, %rsp
    call signal_pending_check
    add $8, %rsp
    /* The syscall result travels back to the user in RAX. A signal handler
       that returned through rt_sigreturn has its own value. */
    mov SYSCALL_RESULT(%rip), %rax
    cmpb $0, SIGRET_VALID(%rip)
    je .Lno_sigret
    mov SIGRET_VALUE(%rip), %rax
.Lno_sigret:
    /* SYSRET loads RFLAGS from RAX, so a SYSRET return would replace the user
       flags with the syscall result. The return uses IRETQ instead: it
       restores RIP, CS, RFLAGS, RSP and SS and leaves RAX alone, which keeps
       the syscall result and the user flags intact. */
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
    /* The saved user RAX is dropped; RAX keeps the dispatcher result. */
    add $8, %rsp
    pop %rcx
    pop %r11
    /* IRETQ frame, pushed in reverse pop order: SS, RSP, RFLAGS, CS, RIP. */
    push USER_SS(%rip)
    push SYSCALL_USER_RSP(%rip)
    push %r11
    push USER_CS(%rip)
    push %rcx
    iretq
.Lbad_pre:
    mov 104(%rsp), %rcx
    mov SYSCALL_USER_RSP(%rip), %rdx
    jmp .Lbad
.Lbad:
    mov %rcx, %rdi
    mov %rdx, %rsi
    mov SYSCALL_KERNEL_RSP(%rip), %rsp
    and $-16, %rsp
    call bad_sysret
#ifdef __ELF__
.size syscall_entry,.-syscall_entry
#endif
