.text
.global isr_default,isr_debug,isr_invalid_tss,isr_segment_not_present,isr_stack_segment
.global isr_general_protection,isr_page_fault,isr_alignment_check,isr_double_fault
/* Exceptions without a CPU error code get a synthetic zero. */
isr_default: pushq $0; pushq $255; jmp common_exception
/* Debug (#DB, vector 1): no error code. The A2 stop trap single-steps a
   user task with a pending stop into here so the stop runs on the task's
   own kernel stack. */
isr_debug: pushq $0; pushq $1; jmp common_exception
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
    /* The faulting instruction is retried after the handler, so the SSE
       register file has to survive the call into Rust unchanged. The kernel
       itself uses XMM registers, and a demand-paging or COW fault on a
       `movups` would otherwise write the handler's leftovers back to user
       memory. */
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
    mov 376(%rsp),%rdi
    mov 384(%rsp),%rsi
    mov 392(%rsp),%rdx
    mov 400(%rsp),%rcx
    lea 392(%rsp),%r8
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
    /* Drop the SSE save area together with the synthetic vector and error
       words; the CPU frame (rip, cs, rflags, ...) follows. */
    addq $272, %rsp
    iretq
