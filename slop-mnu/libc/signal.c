#include <signal.h>
#include <unistd.h>
#include <errno.h>
#include "slop_internal.h"

#define SYS_RT_SIGACTION 13
#define SYS_RT_SIGPROCMASK 14
#define SYS_RT_SIGRETURN 15
#define SYS_KILL 62

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
        kact.flags = act->sa_flags;
        kact.restorer = act->sa_restorer ? act->sa_restorer : __slop_sigrestore;
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

/* Installed as sa_restorer for every handler; the kernel rewrites the saved
   context and returns to the interrupted point through rt_sigreturn. */
void __slop_sigrestore(void) {
    __slop_syscall6(SYS_RT_SIGRETURN, 0, 0, 0, 0, 0, 0);
}
