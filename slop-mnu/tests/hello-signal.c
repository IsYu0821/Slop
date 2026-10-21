#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/wait.h>

static int failures;

#define CHECK(cond, what)                       \
    do {                                        \
        if (!(cond)) {                          \
            puts("signals: failed: " what);     \
            failures++;                         \
        }                                       \
    } while (0)

/* The handler records what the kernel delivered. The variables are volatile
   because the handler runs in an asynchronous context. */
static volatile sig_atomic_t usr1_hits;
static volatile sig_atomic_t usr1_signo;
static volatile sig_atomic_t usr1_code;
static volatile pid_t usr1_sender;
static volatile unsigned long usr1_rip;

static void usr1_handler(int sig, siginfo_t *si, void *uc) {
    usr1_signo = sig;
    if (si) {
        usr1_code = si->si_code;
        usr1_sender = si->_sifields._kill.si_pid;
    }
    if (uc) {
        ucontext_t *ctx = (ucontext_t *)uc;
        usr1_rip = (unsigned long)ctx->uc_mcontext.gregs[REG_RIP];
    }
    usr1_hits++;
}

static void sleep_ms(long ms) {
    struct timespec ts;
    ts.tv_sec = 0;
    ts.tv_nsec = ms * 1000000L;
    nanosleep(&ts, 0);
}

int main(int argc, char **argv, char **envp) {
    struct sigaction sa;
    sigset_t block;
    sigset_t saved;
    sigset_t pending;
    pid_t pid;
    pid_t r;
    int status;
    int hits;
    int i;

    puts("signals: main entered");
    CHECK(argc == 2 && argv[1] != 0 && strcmp(argv[1], "from-posix") == 0, "argv");
    (void)envp;

    /* A handler installed with SA_SIGINFO receives the siginfo and the
       machine context of the interrupted code. */
    sigemptyset(&sa.sa_mask);
    sa.sa_handler = usr1_handler;
    sa.sa_flags = SA_SIGINFO;
    sa.sa_restorer = 0;
    CHECK(sigaction(SIGUSR1, &sa, 0) == 0, "sigaction");

    pid = fork();
    CHECK(pid >= 0, "fork");
    if (pid == 0) {
        kill(getppid(), SIGUSR1);
        _exit(0);
    }
    /* The queued signal interrupts the wait. The handler runs before the
       syscall returns to user space, so the checks below see it. */
    status = 0;
    r = waitpid(pid, &status, 0);
    if (r != pid) {
        status = 0;
        r = waitpid(pid, &status, 0);
    }
    CHECK(r == pid, "child reaped");
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0, "child exit status");
    if (usr1_hits == 0) sleep_ms(2);
    CHECK(usr1_hits == 1, "handler ran once");
    CHECK(usr1_signo == SIGUSR1, "handler signal number");
    CHECK(usr1_code == SI_USER, "siginfo code");
    CHECK(usr1_sender == pid, "siginfo sender");
    CHECK(usr1_rip != 0, "ucontext instruction pointer");

    /* A signal whose action is SIG_IGN is discarded. */
    hits = usr1_hits;
    sigemptyset(&sa.sa_mask);
    sa.sa_handler = (void (*)(int))SIG_IGN;
    sa.sa_flags = 0;
    sa.sa_restorer = 0;
    CHECK(sigaction(SIGUSR2, &sa, 0) == 0, "sigaction ignore");
    pid = fork();
    CHECK(pid >= 0, "fork ignore");
    if (pid == 0) {
        kill(getppid(), SIGUSR2);
        _exit(0);
    }
    status = 0;
    r = waitpid(pid, &status, 0);
    CHECK(r == pid, "ignored child reaped");
    CHECK(usr1_hits == hits, "ignored signal discarded");

    /* A blocked signal stays pending until the mask allows the delivery. */
    hits = usr1_hits;
    sigemptyset(&block);
    sigaddset(&block, SIGUSR1);
    CHECK(sigprocmask(SIG_BLOCK, &block, &saved) == 0, "sigprocmask block");
    pid = fork();
    CHECK(pid >= 0, "fork blocked");
    if (pid == 0) {
        kill(getppid(), SIGUSR1);
        _exit(0);
    }
    status = 0;
    r = waitpid(pid, &status, 0);
    CHECK(r == pid, "blocked child reaped");
    CHECK(usr1_hits == hits, "blocked signal not delivered");
    CHECK(sigpending(&pending) == 0, "sigpending");
    CHECK(sigismember(&pending, SIGUSR1) == 1, "blocked signal pending");
    CHECK(sigprocmask(SIG_SETMASK, &saved, 0) == 0, "sigprocmask restore");
    CHECK(usr1_hits == hits + 1, "queued signal delivered after unblock");

    /* The default action of SIGKILL terminates the child. */
    pid = fork();
    CHECK(pid >= 0, "fork killed");
    if (pid == 0) {
        raise(SIGKILL);
        _exit(0);
    }
    status = 0;
    r = waitpid(pid, &status, 0);
    CHECK(r == pid, "killed child reaped");
    CHECK(WIFSIGNALED(status), "child killed by a signal");
    CHECK(WTERMSIG(status) == SIGKILL, "child signal number");

    /* A stopped process continues when SIGCONT arrives. The parent keeps
       sending SIGCONT because the signal may race with the stop. */
    pid = fork();
    CHECK(pid >= 0, "fork stopped");
    if (pid == 0) {
        raise(SIGSTOP);
        _exit(3);
    }
    status = 0;
    r = 0;
    for (i = 0; i < 500; i++) {
        kill(pid, SIGCONT);
        sleep_ms(1);
        r = waitpid(pid, &status, WNOHANG);
        if (r == pid) break;
    }
    CHECK(r == pid, "continued child reaped");
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 3, "stopped child continued");

    if (failures) {
        printf("signals: %d checks failed\n", failures);
        return 1;
    }
    puts("signals: handler, mask, ignore, default and stop/continue passed");
    return 0;
}
