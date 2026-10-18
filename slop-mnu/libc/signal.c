#include <signal.h>
#include <unistd.h>

int kill(int pid, int sig) {
    return __slop_syscall6(62, pid, sig, 0, 0, 0, 0);
}

int sigaction(int sig, const struct sigaction *act, struct sigaction *oldact) {
    return __slop_syscall6(13, sig, (long)act, (long)oldact, 0, 0, 0);
}

int sigprocmask(int how, const sigset_t *set, sigset_t *oldset) {
    return __slop_syscall6(14, how, (long)set, (long)oldset, 0, 0, 0);
}

int sigreturn(void) {
    return __slop_syscall6(15, 0, 0, 0, 0, 0, 0);
}
