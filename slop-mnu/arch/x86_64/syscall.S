.text
.global syscall_entry
.type syscall_entry,@function
syscall_entry:
    /* SYSCALL leaves user RIP in RCX and RFLAGS in R11; it does not switch RSP. */
    mov %rsp, SYSCALL_USER_RSP(%rip)
    mov SYSCALL_KERNEL_RSP(%rip), %rsp
    push %r11
    push %rcx
    push %rbx
    push %rbp
    push %r12
    push %r13
    push %r14
    push %r15
    /* Linux ABI: rax=nr, rdi,rsi,rdx,r10,r8,r9=args. SysV call has arg7 on stack. */
    push $0
    push %r9
    mov %r8, %r9
    mov %r10, %r8
    mov %rdx, %rcx
    mov %rsi, %rdx
    mov %rdi, %rsi
    mov %rax, %rdi
    call syscall_dispatch_raw
    add $16, %rsp
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbp
    pop %rbx
    pop %rcx
    pop %r11
    cmpb $0, EXEC_PENDING(%rip)
    je .Lnormal_return
    mov EXEC_RIP(%rip), %rcx
    movb $0, EXEC_PENDING(%rip)
.Lnormal_return:
    mov SYSCALL_USER_RSP(%rip), %rsp
    /* SYSRET requires canonical lower-half addresses. */
    mov %rcx, %rdx
    shr $47, %rdx
    jne .Lbad
    mov %rsp, %rdx
    shr $47, %rdx
    jne .Lbad
    sysretq
.Lbad:
    mov %rcx, %rdi
    mov %rsp, %rsi
    mov SYSCALL_KERNEL_RSP(%rip), %rsp
    and $-16, %rsp
    call bad_sysret
.size syscall_entry,.-syscall_entry
