/* Minimal /init: execve the acceptance binary, then halt on failure.
 * Default target is /bin/hello. With -DMODTEST_INIT the image boots into
 * /bin/modtest instead (Linux .ko load/unload + EDU device vertical).
 * Both paths stay in this file so check-source token checks keep working. */
.global _start
.section .text
_start:
    mov $59,%rax
#ifdef MODTEST_INIT
    lea modtest_path(%rip), %rdi
    lea modtest_argv(%rip),%rsi
#else
    lea hello_path(%rip),%rdi
    lea argv(%rip),%rsi
#endif
    lea envp(%rip),%rdx
    syscall
failure:
    mov $1,%rax; mov $2,%rdi; lea failed(%rip),%rsi; mov $failed_end-failed,%rdx; syscall
    mov $231,%rax; mov $1,%rdi; syscall
1:  jmp 1b
.section .rodata
hello_path: .asciz "/bin/hello"
arg0: .asciz "hello"
modtest_path: .asciz "/bin/modtest"
modtest_arg0: .asciz "modtest"
env0: .asciz "SLOP_MNU_EXEC=1"
.align 8
argv: .quad arg0,0
modtest_argv: .quad modtest_arg0,0
envp: .quad env0,0
failed: .ascii "Slop/MNU execve acceptance failed\n"
failed_end:
