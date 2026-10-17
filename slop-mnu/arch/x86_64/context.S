.text
.global switch_context
#ifdef __ELF__
.type switch_context,@function
#endif
/* SysV64: RDI=&old_rsp, RSI=new_rsp. Saves callee-preserved state. */
switch_context:
    push %rbp
    push %rbx
    push %r12
    push %r13
    push %r14
    push %r15
    mov %rsp,(%rdi)
    mov %rsi,%rsp
    pop %r15
    pop %r14
    pop %r13
    pop %r12
    pop %rbx
    pop %rbp
    ret
#ifdef __ELF__
.size switch_context,.-switch_context
#endif
