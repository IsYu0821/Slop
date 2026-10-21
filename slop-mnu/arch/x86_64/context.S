.text
.global switch_context
#ifdef __ELF__
.type switch_context,@function
#endif
/* SysV64: RDI=&old_rsp, RSI=new_rsp, RDX=old_fpu_area, RCX=new_fpu_area.
   Saves callee-preserved state and the SSE register file, so code that uses
   XMM registers keeps its values across a context switch. The area holds
   the sixteen 16-byte registers; FXSAVE is avoided because it traps on
   several TCG builds. */
switch_context:
    push %rbp
    push %rbx
    push %r12
    push %r13
    push %r14
    push %r15
    mov %rsp,(%rdi)
    movups %xmm0,0(%rdx)
    movups %xmm1,16(%rdx)
    movups %xmm2,32(%rdx)
    movups %xmm3,48(%rdx)
    movups %xmm4,64(%rdx)
    movups %xmm5,80(%rdx)
    movups %xmm6,96(%rdx)
    movups %xmm7,112(%rdx)
    movups %xmm8,128(%rdx)
    movups %xmm9,144(%rdx)
    movups %xmm10,160(%rdx)
    movups %xmm11,176(%rdx)
    movups %xmm12,192(%rdx)
    movups %xmm13,208(%rdx)
    movups %xmm14,224(%rdx)
    movups %xmm15,240(%rdx)
    mov %rsi,%rsp
    movups 0(%rcx),%xmm0
    movups 16(%rcx),%xmm1
    movups 32(%rcx),%xmm2
    movups 48(%rcx),%xmm3
    movups 64(%rcx),%xmm4
    movups 80(%rcx),%xmm5
    movups 96(%rcx),%xmm6
    movups 112(%rcx),%xmm7
    movups 128(%rcx),%xmm8
    movups 144(%rcx),%xmm9
    movups 160(%rcx),%xmm10
    movups 176(%rcx),%xmm11
    movups 192(%rcx),%xmm12
    movups 208(%rcx),%xmm13
    movups 224(%rcx),%xmm14
    movups 240(%rcx),%xmm15
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

.text
.global fpu_save_regs
#ifdef __ELF__
.type fpu_save_regs,@function
#endif
/* rdi = area; copies the current SSE register file into it. */
fpu_save_regs:
    movups %xmm0,0(%rdi)
    movups %xmm1,16(%rdi)
    movups %xmm2,32(%rdi)
    movups %xmm3,48(%rdi)
    movups %xmm4,64(%rdi)
    movups %xmm5,80(%rdi)
    movups %xmm6,96(%rdi)
    movups %xmm7,112(%rdi)
    movups %xmm8,128(%rdi)
    movups %xmm9,144(%rdi)
    movups %xmm10,160(%rdi)
    movups %xmm11,176(%rdi)
    movups %xmm12,192(%rdi)
    movups %xmm13,208(%rdi)
    movups %xmm14,224(%rdi)
    movups %xmm15,240(%rdi)
    ret
#ifdef __ELF__
.size fpu_save_regs,.-fpu_save_regs
#endif
