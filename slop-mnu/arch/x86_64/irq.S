.text
.global apic_timer_entry
#ifdef __ELF__
.type apic_timer_entry,@function
#endif
apic_timer_entry:
    cld
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
    sub $8, %rsp
    call apic_timer_rust
    add $8, %rsp
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
    iretq
#ifdef __ELF__
.size apic_timer_entry,.-apic_timer_entry
#endif
