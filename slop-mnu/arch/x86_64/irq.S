.text
.global apic_timer_entry
#ifdef __ELF__
.type apic_timer_entry,@function
#endif
apic_timer_entry:
    cld
    /* An interrupt can preempt user code and return to it, and it can also
       preempt kernel code in the middle of an SSE computation. Save the SSE
       register file around the handler; context switches save their own
       copy, but they cannot recover what the handler has already clobbered. */
    sub $256, %rsp
    movups %xmm0,0(%rsp)
    movups %xmm1,16(%rsp)
    movups %xmm2,32(%rsp)
    movups %xmm3,48(%rsp)
    movups %xmm4,64(%rsp)
    movups %xmm5,80(%rsp)
    movups %xmm6,96(%rsp)
    movups %xmm7,112(%rsp)
    movups %xmm8,128(%rsp)
    movups %xmm9,144(%rsp)
    movups %xmm10,160(%rsp)
    movups %xmm11,176(%rsp)
    movups %xmm12,192(%rsp)
    movups %xmm13,208(%rsp)
    movups %xmm14,224(%rsp)
    movups %xmm15,240(%rsp)
    push %rax
    push %rcx
    push %rdx
    push %rsi
    push %rdi
    push %r8
    push %r9
    push %r10
    push %r11
    push %rbx
    push %rbp
    push %r12
    push %r13
    push %r14
    push %r15
    mov %rsp, %r15
    /* DIAG-MS93: record entry RSP if GS is live (zero before bringup). */
    mov $0xc0000101, %ecx
    rdmsr
    test %edx, %edx
    jne 1f
    test %eax, %eax
    je 2f
1:  movq %r15, %gs:88
2:
    /* The vector register save area and the pushed registers lie between
       the stack pointer and the frame of the processor, so the code
       segment is 384 bytes above the stack pointer: 256 bytes of vector
       registers, 120 bytes of general registers, then the program counter
       and the code segment. */
    mov 384(%r15), %rdi
    and $-16, %rsp
    call apic_timer_rust
    mov %r15, %rsp
    /* A2 stop trap: if the handler armed a stop for the interrupted user
       task, set TF so it traps into #DB immediately after IRET, where the
       stop runs on the task's own kernel stack (IST handlers cannot
       schedule). Frame: CS at 384(%r15), RFLAGS at 392(%r15).
       Skip entirely before GS is live (same guard as the entry path). */
    mov $0xc0000101, %ecx
    rdmsr
    test %edx, %edx
    jne 8f
    test %eax, %eax
    je 9f
8:  cmpb $0, %gs:96
    je 9f
    movq 384(%r15), %rax
    andq $3, %rax
    cmpq $3, %rax
    jne 9f
    movb $0, %gs:96
    orq $0x100, 392(%r15)
9:
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbp
    pop %rbx
    pop %r11
    pop %r10
    pop %r9
    pop %r8
    pop %rdi
    pop %rsi
    pop %rdx
    pop %rcx
    pop %rax
    /* The save area lies at the stack pointer the pops have left, because
       the vector registers were saved from that place. */
    movups 0(%rsp),%xmm0
    movups 16(%rsp),%xmm1
    movups 32(%rsp),%xmm2
    movups 48(%rsp),%xmm3
    movups 64(%rsp),%xmm4
    movups 80(%rsp),%xmm5
    movups 96(%rsp),%xmm6
    movups 112(%rsp),%xmm7
    movups 128(%rsp),%xmm8
    movups 144(%rsp),%xmm9
    movups 160(%rsp),%xmm10
    movups 176(%rsp),%xmm11
    movups 192(%rsp),%xmm12
    movups 208(%rsp),%xmm13
    movups 224(%rsp),%xmm14
    movups 240(%rsp),%xmm15
    add $256, %rsp
    iretq
#ifdef __ELF__
.size apic_timer_entry,.-apic_timer_entry
#endif

.text
.global apic_resched_entry
#ifdef __ELF__
.type apic_resched_entry,@function
#endif
apic_resched_entry:
    cld
    /* A reschedule IPI can preempt kernel code with live SSE state, so the
       register file is saved around the handler exactly like the timer. */
    sub $256, %rsp
    movups %xmm0,0(%rsp)
    movups %xmm1,16(%rsp)
    movups %xmm2,32(%rsp)
    movups %xmm3,48(%rsp)
    movups %xmm4,64(%rsp)
    movups %xmm5,80(%rsp)
    movups %xmm6,96(%rsp)
    movups %xmm7,112(%rsp)
    movups %xmm8,128(%rsp)
    movups %xmm9,144(%rsp)
    movups %xmm10,160(%rsp)
    movups %xmm11,176(%rsp)
    movups %xmm12,192(%rsp)
    movups %xmm13,208(%rsp)
    movups %xmm14,224(%rsp)
    movups %xmm15,240(%rsp)
    push %rax
    push %rcx
    push %rdx
    push %rsi
    push %rdi
    push %r8
    push %r9
    push %r10
    push %r11
    push %rbx
    push %rbp
    push %r12
    push %r13
    push %r14
    push %r15
    mov %rsp, %r15
    /* DIAG-MS93: record entry RSP if GS is live (zero before bringup). */
    mov $0xc0000101, %ecx
    rdmsr
    test %edx, %edx
    jne 1f
    test %eax, %eax
    je 3f
1:      movq %r15, %gs:88
3:
    mov 384(%r15), %rdi
    and $-16, %rsp
    call apic_resched_rust
    mov %r15, %rsp
    /* A2 stop trap: same as the timer entry (CS at 384, RFLAGS at 392),
       with the same before-GS-liveness guard. */
    mov $0xc0000101, %ecx
    rdmsr
    test %edx, %edx
    jne 8f
    test %eax, %eax
    je 9f
8:  cmpb $0, %gs:96
    je 9f
    movq 384(%r15), %rax
    andq $3, %rax
    cmpq $3, %rax
    jne 9f
    movb $0, %gs:96
    orq $0x100, 392(%r15)
9:
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbp
    pop %rbx
    pop %r11
    pop %r10
    pop %r9
    pop %r8
    pop %rdi
    pop %rsi
    pop %rdx
    pop %rcx
    pop %rax
    movups 0(%rsp),%xmm0
    movups 16(%rsp),%xmm1
    movups 32(%rsp),%xmm2
    movups 48(%rsp),%xmm3
    movups 64(%rsp),%xmm4
    movups 80(%rsp),%xmm5
    movups 96(%rsp),%xmm6
    movups 112(%rsp),%xmm7
    movups 128(%rsp),%xmm8
    movups 144(%rsp),%xmm9
    movups 160(%rsp),%xmm10
    movups 176(%rsp),%xmm11
    movups 192(%rsp),%xmm12
    movups 208(%rsp),%xmm13
    movups 224(%rsp),%xmm14
    movups 240(%rsp),%xmm15
    add $256, %rsp
    iretq
#ifdef __ELF__
.size apic_resched_entry,.-apic_resched_entry
#endif
