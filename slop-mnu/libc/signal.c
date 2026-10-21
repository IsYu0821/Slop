#include <signal.h>
#include <unistd.h>
#include <errno.h>
#include "slop_internal.h"

#define SYS_RT_SIGACTION 13
#define SYS_RT_SIGPROCMASK 14
#define SYS_RT_SIGRETURN 15
#define SYS_KILL 62
#define SYS_RT_SIGPENDING 127

/* POSIX wrapper convention: report -1 and stash the raw errno value. */
static long sig_fail(long r) {
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return r;
}

int kill(int pid, int sig) {
    return (int)sig_fail(__slop_syscall6(SYS_KILL, pid, sig, 0, 0, 0, 0));
}

int sigaction(int sig, const struct sigaction *act, struct sigaction *oldact) {
    struct kernel_sigaction kact;
    struct kernel_sigaction kold;
    struct kernel_sigaction *kactp = 0;
    struct kernel_sigaction *koldp = 0;

    if (act) {
        kact.handler = act->sa_handler;
        /* The x86-64 ABI needs a restorer: the handler returns into it and
           it performs the rt_sigreturn syscall. */
        kact.flags = act->sa_flags | SA_RESTORER;
        kact.restorer = act->sa_restorer ? act->sa_restorer : __slop_sigreturn_trampoline;
        kact.mask = act->sa_mask;
        kactp = &kact;
    }
    if (oldact) koldp = &kold;

    long r = sig_fail(__slop_syscall6(SYS_RT_SIGACTION, sig,
                                      (long)kactp, (long)koldp, 0, 0, 0));
    if (r < 0) return -1;

    if (oldact) {
        oldact->sa_handler = kold.handler;
        oldact->sa_mask = kold.mask;
        oldact->sa_flags = kold.flags;
        oldact->sa_restorer = kold.restorer;
    }
    return 0;
}

int sigprocmask(int how, const sigset_t *set, sigset_t *oldset) {
    return (int)sig_fail(__slop_syscall6(SYS_RT_SIGPROCMASK, how, (long)set,
                                         (long)oldset, 0, 0, 0));
}

/* The restorer trampoline lives in sigreturn.S: it must enter the kernel
   without touching the stack, because the kernel reads the signal frame
   from the stack pointer the handler returned with. */
void __slop_sigreturn_trampoline(void);

int raise(int sig) {
    return kill(getpid(), sig);
}

int sigpending(sigset_t *set) {
    long r = sig_fail(__slop_syscall6(SYS_RT_SIGPENDING, (long)set, 8, 0, 0, 0, 0));
    return (int)r;
}

int sigemptyset(sigset_t *set) {
    *set = 0;
    return 0;
}

int sigaddset(sigset_t *set, int sig) {
    if (sig < 1 || sig > 63) return -1;
    *set |= 1UL << (sig - 1);
    return 0;
}

int sigdelset(sigset_t *set, int sig) {
    if (sig < 1 || sig > 63) return -1;
    *set &= ~(1UL << (sig - 1));
    return 0;
}

int sigismember(const sigset_t *set, int sig) {
    if (sig < 1 || sig > 63) return -1;
    return (int)((*set >> (sig - 1)) & 1UL);
}

void sigreturn(void) {
    __slop_syscall6(SYS_RT_SIGRETURN, 0, 0, 0, 0, 0, 0);
}
