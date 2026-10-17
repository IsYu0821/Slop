/* Freestanding C startup. The kernel stack builder places argc at (%rsp),
   argv right after it and envp after the argv NULL terminator. */
.global _start
.type _start,@function
_start:
    xor %rbp, %rbp
    mov (%rsp), %rdi
    lea 8(%rsp), %rsi
    lea 16(%rsp,%rdi,8), %rdx
    mov %rdx, environ(%rip)
    and $-16, %rsp
    call main
    mov %eax, %edi
    mov $231, %eax
    syscall
1:  jmp 1b
.size _start, .-_start
