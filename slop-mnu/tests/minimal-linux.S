.global _start
.section .text
_start:
    mov $59,%rax
    lea hello_path(%rip),%rdi
    lea argv(%rip),%rsi
    lea envp(%rip),%rdx
    syscall
failure:
    mov $1,%rax; mov $2,%rdi; lea failed(%rip),%rsi; mov $failed_end-failed,%rdx; syscall
    mov $231,%rax; mov $1,%rdi; syscall
1:  jmp 1b
.section .rodata
hello_path: .asciz "/bin/hello"
arg0: .asciz "hello"
env0: .asciz "SLOP_MNU_EXEC=1"
.align 8
argv: .quad arg0,0
envp: .quad env0,0
failed: .ascii "Slop/MNU execve acceptance failed\n"
failed_end:
