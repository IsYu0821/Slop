/* long __slop_syscall6(long n, long a1, long a2, long a3, long a4, long a5, long a6)
   Translates the System V C argument registers to the Linux x86-64 syscall ABI. */
.global __slop_syscall6
.type __slop_syscall6,@function
__slop_syscall6:
    mov %rdi, %rax
    mov %rsi, %rdi
    mov %rdx, %rsi
    mov %rcx, %rdx
    mov %r8, %r10
    mov %r9, %r8
    mov 8(%rsp), %r9
    syscall
    ret
.size __slop_syscall6, .-__slop_syscall6
