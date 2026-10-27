.text
.global syscall_entry
#ifdef __ELF__
.type syscall_entry,@function
#endif
syscall_entry:
    /* SYSCALL leaves user RIP in RCX and RFLAGS in R11; it does not switch RSP.
       GS_BASE is pinned to the per-CPU block by boot (userspace never owns
       GS here: no ARCH_SET_GS), so no swapgs is needed and %gs always
       addresses the running CPU's state, in any ring. */
    mov %rsp, %gs:16
    mov %gs:8, %rsp
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
    mov %rsp, %gs:24
    /* The kernel uses XMM registers, so the user SSE file is parked in the
       running task's FPU area on entry and restored on exit. The area is
       per-task, so a task blocked inside a syscall keeps its registers while
       another task runs. FXSAVE is avoided because it traps on several TCG
       builds; the sixteen registers are moved directly. */
    mov %gs:0, %r11
    test %r11, %r11
    jz .Lfpu_saved
    movups %xmm0,0(%r11)
    movups %xmm1,16(%r11)
    movups %xmm2,32(%r11)
    movups %xmm3,48(%r11)
    movups %xmm4,64(%r11)
    movups %xmm5,80(%r11)
    movups %xmm6,96(%r11)
    movups %xmm7,112(%r11)
    movups %xmm8,128(%r11)
    movups %xmm9,144(%r11)
    movups %xmm10,160(%r11)
    movups %xmm11,176(%r11)
    movups %xmm12,192(%r11)
    movups %xmm13,208(%r11)
    movups %xmm14,224(%r11)
    movups %xmm15,240(%r11)
    /* The user SSE file now lives in the task area; live XMM is kernel
       scratch until the return path restores it. A context switch in
       between must divert its save to scratch (see fpu_parked). */
    movb $1, %gs:104
.Lfpu_saved:
    mov 96(%rsp), %rdi
    mov 88(%rsp), %rsi
    mov 80(%rsp), %rdx
    mov 72(%rsp), %rcx
    mov 48(%rsp), %r8
    mov 64(%rsp), %r9
    push 56(%rsp)
    movb $0, %gs:72
    call syscall_dispatch_raw
    add $8, %rsp
    /* The dispatcher result travels back to the user in RAX. The saved RAX
       slot keeps the syscall number, so an interrupted syscall can be
       restarted with the same number. The result is parked in a scratch
       cell because the signal check below is a call. */
    mov %rax, %gs:32
    cmpb $0, %gs:73
    je .Lcheck_signal
    /* An execve redirects the return to the new image. */
    mov %gs:40, %rdx
    mov %rdx, 104(%rsp)
    movb $0, %gs:73
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
    mov %gs:32, %rax
    cmpb $0, %gs:72
    je .Lno_sigret
    mov %gs:48, %rax
.Lno_sigret:
    /* The FPU area may have been updated by rt_sigreturn, so the exit
       restores whatever the running task now owns. */
    mov %gs:0, %r11
    test %r11, %r11
    jz .Lfpu_restored
    movups 0(%r11),%xmm0
    movups 16(%r11),%xmm1
    movups 32(%r11),%xmm2
    movups 48(%r11),%xmm3
    movups 64(%r11),%xmm4
    movups 80(%r11),%xmm5
    movups 96(%r11),%xmm6
    movups 112(%r11),%xmm7
    movups 128(%r11),%xmm8
    movups 144(%r11),%xmm9
    movups 160(%r11),%xmm10
    movups 176(%r11),%xmm11
    movups 192(%r11),%xmm12
    movups 208(%r11),%xmm13
    movups 224(%r11),%xmm14
    movups 240(%r11),%xmm15
    /* Restored: live XMM is user state again until the next park. */
    movb $0, %gs:104
.Lfpu_restored:
    /* SYSRET loads RFLAGS from RAX, so a SYSRET return would replace the user
       flags with the syscall result. The return uses IRETQ instead: it
       restores RIP, CS, RFLAGS, RSP and SS and leaves RAX alone, which keeps
       the syscall result and the user flags intact. */
    mov 104(%rsp), %rdx
    shr $47, %rdx
    jne .Lbad_pre
    mov %gs:16, %rdx
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
    push %gs:16
    push %r11
    push USER_CS(%rip)
    push %rcx
    iretq
.Lbad_pre:
    mov 104(%rsp), %rcx
    mov %gs:16, %rdx
    jmp .Lbad
.Lbad:
    mov %rcx, %rdi
    mov %rdx, %rsi
    mov %gs:8, %rsp
    and $-16, %rsp
    call bad_sysret
#ifdef __ELF__
.size syscall_entry,.-syscall_entry
#endif
