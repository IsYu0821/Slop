#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>
#include <sys/wait.h>

static int failures;

#define CHECK(cond, what)                       \
    do {                                        \
        if (!(cond)) {                          \
            puts("signals: failed: " what);     \
            failures++;                         \
        }                                       \
    } while (0)

static volatile sig_atomic_t usr1_seen;
static volatile sig_atomic_t usr2_seen;

static void usr1_handler(int sig) {
    usr1_seen = sig;
}

static void usr2_handler(int sig) {
    usr2_seen = sig;
}

int main(int argc, char **argv, char **envp) {
    struct sigaction sa;
    struct sigaction old;
    sigset_t set;
    sigset_t oldset;
    pid_t pid;
    int status;

    (void)argc; (void)argv; (void)envp;
    puts("signals: main entered");

    /* 1. rt_sigaction installs a handler and reports the previous one. */
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = usr1_handler;
    CHECK(sigaction(SIGUSR1, &sa, &old) == 0, "sigaction");
    CHECK(old.sa_handler == SIG_DFL, "sigaction reports SIG_DFL");
    CHECK(old.sa_flags == 0, "sigaction old flags");
    CHECK(old.sa_mask == 0, "sigaction old mask");
    /* The wrapper installs the return trampoline automatically. */
    CHECK(sigaction(SIGUSR1, 0, &old) == 0, "sigaction query");
    CHECK(old.sa_handler == usr1_handler, "sigaction handler stored");
    CHECK(old.sa_restorer != 0, "sigaction restorer installed");

    /* 2. A signal sent to ourselves is delivered at a syscall boundary. */
    usr1_seen = 0;
    CHECK(kill(getpid(), SIGUSR1) == 0, "kill self");
    /* Delivery happens at the syscall boundary right after kill returns. */
    puts("signals: boundary");
    CHECK(usr1_seen == SIGUSR1, "handler ran at boundary");

    /* 3. Blocked signals stay pending until unblocked. */
    set = 1UL << (SIGUSR1 - 1);
    CHECK(sigprocmask(SIG_BLOCK, &set, &oldset) == 0, "sigprocmask block");
    usr1_seen = 0;
    CHECK(kill(getpid(), SIGUSR1) == 0, "kill self blocked");
    puts("signals: blocked");
    CHECK(usr1_seen == 0, "blocked signal not delivered");
    CHECK(sigprocmask(SIG_SETMASK, &oldset, 0) == 0, "sigprocmask restore");
    puts("signals: unblocked");
    CHECK(usr1_seen == SIGUSR1, "signal delivered after unblock");

    /* 4. A parent signal reaches a blocked child, which then exits 7. */
    pid = fork();
    CHECK(pid >= 0, "fork");
    if (pid == 0) {
        struct sigaction csa;
        memset(&csa, 0, sizeof(csa));
        csa.sa_handler = usr2_handler;
        CHECK(sigaction(SIGUSR2, &csa, 0) == 0, "child sigaction");
        for (int i = 0; i < 50 && !usr2_seen; i++) {
            struct timespec ts = {0, 1000000};
            nanosleep(&ts, 0);
        }
        if (usr2_seen != SIGUSR2) _exit(9);
        _exit(7);
    }
    CHECK(kill(pid, SIGUSR2) == 0, "kill child");
    CHECK(waitpid(pid, &status, 0) == pid, "waitpid child");
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 7, "child ran handler");

    /* 5. Default action: SIGTERM terminates the child. */
    pid = fork();
    CHECK(pid >= 0, "fork for default");
    if (pid == 0) {
        kill(getpid(), SIGTERM);
        puts("signals: unreachable");
        _exit(9);
    }
    CHECK(waitpid(pid, &status, 0) == pid, "waitpid default");
    CHECK(WIFSIGNALED(status), "default terminated by signal");
    CHECK(WTERMSIG(status) == SIGTERM, "default terminated by SIGTERM");

    if (failures) {
        printf("signals: %d checks failed\n", failures);
        return 1;
    }
    puts("signals: handler delivery, mask and default action passed");
    return 0;
}
