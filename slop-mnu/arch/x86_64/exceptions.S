.text
.global isr_default,isr_invalid_tss,isr_segment_not_present,isr_stack_segment
.global isr_general_protection,isr_page_fault,isr_alignment_check,isr_double_fault
/* Exceptions without a CPU error code get a synthetic zero. */
isr_default: pushq $0; pushq $255; jmp common_exception
/* Exceptions below already have a CPU-pushed error code. */
isr_double_fault: pushq $8; jmp common_exception
isr_invalid_tss: pushq $10; jmp common_exception
isr_segment_not_present: pushq $11; jmp common_exception
isr_stack_segment: pushq $12; jmp common_exception
isr_general_protection: pushq $13; jmp common_exception
isr_page_fault: pushq $14; jmp common_exception
isr_alignment_check: pushq $17; jmp common_exception
/* The CPU saves rip/cs/rflags/rsp/ss on privilege transitions but not the
   general-purpose registers. Demand paging and COW resume user code through
   iretq after handling a fault, so every GPR the handler may clobber has to be
   saved here and restored before returning. */
common_exception:
    pushq %rax
    pushq %rbx
    pushq %rcx
    pushq %rdx
    pushq %rsi
    pushq %rdi
    pushq %rbp
    pushq %r8
    pushq %r9
    pushq %r10
    pushq %r11
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15
    mov 120(%rsp),%rdi
    mov 128(%rsp),%rsi
    mov 136(%rsp),%rdx
    mov 144(%rsp),%rcx
    mov %rsp,%rbp
    and $-16,%rsp
    cld
    call exception_rust
    mov %rbp,%rsp
    popq %r15
    popq %r14
    popq %r13
    popq %r12
    popq %r11
    popq %r10
    popq %r9
    popq %r8
    popq %rbp
    popq %rdi
    popq %rsi
    popq %rdx
    popq %rcx
    popq %rbx
    popq %rax
    addq $16, %rsp
    iretq
