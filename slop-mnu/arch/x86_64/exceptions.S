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
common_exception:
    mov 0(%rsp),%rdi
    mov 8(%rsp),%rsi
    mov 16(%rsp),%rdx
    mov 24(%rsp),%rcx
    and $-16,%rsp
    cld
    call exception_rust
