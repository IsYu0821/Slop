#ifndef _SLOP_SIGNAL_H
#define _SLOP_SIGNAL_H

#include <stddef.h>

#ifndef _SLOP_PID_T
#define _SLOP_PID_T
typedef int pid_t;
#endif

typedef unsigned long sigset_t;
typedef int sig_atomic_t;

#define SIGHUP 1
#define SIGINT 2
#define SIGQUIT 3
#define SIGILL 4
#define SIGTRAP 5
#define SIGABRT 6
#define SIGBUS 7
#define SIGFPE 8
#define SIGKILL 9
#define SIGUSR1 10
#define SIGSEGV 11
#define SIGUSR2 12
#define SIGPIPE 13
#define SIGALRM 14
#define SIGTERM 15
#define SIGCHLD 17
#define SIGCONT 18
#define SIGSTOP 19
#define SIGTSTP 20

#define SIG_DFL 0
#define SIG_IGN 1

#define SA_NOCLDSTOP 1
#define SA_NOCLDWAIT 2
#define SA_SIGINFO 4
#define SA_ONSTACK 0x08000000
#define SA_RESTORER 0x04000000
#define SA_RESTART 0x10000000
#define SA_NODEFER 0x40000000
#define SA_RESETHAND 0x80000000

#define SIG_BLOCK 0
#define SIG_UNBLOCK 1
#define SIG_SETMASK 2

/* The sender of a queued signal was a process. */
#define SI_USER 0
#define SI_KERNEL 0x80

/* The POSIX userspace view of a signal action. The kernel expects the
   k_sigaction layout, so the wrapper translates between the two. */
struct sigaction {
    void (*sa_handler)(int);
    sigset_t sa_mask;
    int sa_flags;
    void (*sa_restorer)(void);
};

/* Linux x86-64 k_sigaction: the layout the rt_sigaction syscall takes. The
   field order is part of the ABI. */
struct kernel_sigaction {
    void (*handler)(int);
    unsigned long flags;
    void (*restorer)(void);
    sigset_t mask;
};

/* Linux siginfo_t: the header followed by the 128-byte union area. */
typedef struct {
    int si_signo;
    int si_errno;
    int si_code;
    union {
        int _pad[29];
        struct {
            pid_t si_pid;
            unsigned int si_uid;
        } _kill;
    } _sifields;
} siginfo_t;

/* The x86-64 machine context: the register array, the floating-point state
   pointer and the reserved words. The register order is the published REG_*
   index list. */
typedef unsigned long greg_t;
typedef greg_t gregset_t[23];
typedef struct _libc_fpstate *fpregset_t;
typedef struct {
    gregset_t gregs;
    fpregset_t fpregs;
    unsigned long long __reserved1[8];
} mcontext_t;

typedef struct {
    void *ss_sp;
    int ss_flags;
    size_t ss_size;
} stack_t;

typedef struct ucontext_t {
    unsigned long uc_flags;
    struct ucontext_t *uc_link;
    stack_t uc_stack;
    mcontext_t uc_mcontext;
    sigset_t uc_sigmask;
    unsigned char __fpregs_mem[512];
} ucontext_t;

#define REG_R8 0
#define REG_R9 1
#define REG_R10 2
#define REG_R11 3
#define REG_R12 4
#define REG_R13 5
#define REG_R14 6
#define REG_R15 7
#define REG_RDI 8
#define REG_RSI 9
#define REG_RBP 10
#define REG_RBX 11
#define REG_RDX 12
#define REG_RAX 13
#define REG_RCX 14
#define REG_RSP 15
#define REG_RIP 16
#define REG_EFL 17
#define REG_CSGSFS 18
#define REG_ERR 19
#define REG_TRAPNO 20
#define REG_OLDMASK 21
#define REG_CR2 22

/* The signal handler restorer: the handler returns into it and it performs
   the rt_sigreturn syscall so the kernel restores the interrupted context. */
void __slop_sigreturn_trampoline(void);

int kill(pid_t pid, int sig);
int raise(int sig);
int sigaction(int sig, const struct sigaction *act, struct sigaction *oldact);
int sigprocmask(int how, const sigset_t *set, sigset_t *oldset);
int sigpending(sigset_t *set);
int sigemptyset(sigset_t *set);
int sigaddset(sigset_t *set, int sig);
int sigdelset(sigset_t *set, int sig);
int sigismember(const sigset_t *set, int sig);
void sigreturn(void);

#endif
