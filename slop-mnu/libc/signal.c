#include <signal.h>
#include <unistd.h>
#include <errno.h>
#include "slop_internal.h"

#define SYS_KILL 62
#define SYS_RT_SIGACTION 13
#define SYS_RT_SIGPROCMASK 14
#define SYS_RT_SIGRETURN 15
#define SYS_RT_SIGPENDING 127

/* The kernel action layout: handler, flags, mask and restorer. The user
   visible struct sigaction keeps the POSIX order, so the wrapper translates. */
struct k_sigaction {
    void (*k_handler)(int);
    unsigned long k_flags;
    sigset_t k_mask;
    void (*k_restorer)(void);
};

/* The handler returns into this address, which performs the rt_sigreturn
   syscall and lets the kernel restore the interrupted context. */
extern void __slop_sigreturn_trampoline(void);

int kill(pid_t pid, int sig) {
    long r = __slop_syscall6(SYS_KILL, (long)pid, (long)sig, 0, 0, 0, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

int raise(int sig) {
    return kill(getpid(), sig);
}

int sigaction(int sig, const struct sigaction *act, struct sigaction *oldact) {
    struct k_sigaction ka = {0, 0, 0, 0};
    struct k_sigaction ko = {0, 0, 0, 0};
    long r;
    if (act) {
        ka.k_handler = act->sa_handler;
        ka.k_flags = ((unsigned long)act->sa_flags & ~(unsigned long)SA_RESTORER) | SA_RESTORER;
        ka.k_mask = act->sa_mask;
        ka.k_restorer = act->sa_restorer ? act->sa_restorer : __slop_sigreturn_trampoline;
    }
    r = __slop_syscall6(SYS_RT_SIGACTION, (long)sig, act ? (long)&ka : 0,
                         oldact ? (long)&ko : 0, 0, 0, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    if (oldact) {
        oldact->sa_handler = ko.k_handler;
        oldact->sa_mask = ko.k_mask;
        oldact->sa_flags = (int)ko.k_flags;
        oldact->sa_restorer = ko.k_restorer;
    }
    return 0;
}

int sigprocmask(int how, const sigset_t *set, sigset_t *oldset) {
    long r = __slop_syscall6(SYS_RT_SIGPROCMASK, (long)how, (long)set, (long)oldset, 0, 0, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
}

int sigpending(sigset_t *set) {
    long r = __slop_syscall6(SYS_RT_SIGPENDING, (long)set, 8, 0, 0, 0, 0);
    if (r < 0) {
        errno = (int)-r;
        return -1;
    }
    return 0;
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
