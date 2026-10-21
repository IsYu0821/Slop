/* The signal handler restorer. The kernel stores this address at the bottom of
   the signal frame, so a handler that returns with `ret` lands here and the
   rt_sigreturn syscall restores the interrupted context. */
.global __slop_sigreturn_trampoline
.type __slop_sigreturn_trampoline,@function
__slop_sigreturn_trampoline:
    mov $15, %eax              /* rt_sigreturn */
    syscall
    /* The kernel restores the interrupted context, so the syscall never
       returns here. */
1:  jmp 1b
.size __slop_sigreturn_trampoline,.-__slop_sigreturn_trampoline
