#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>

static int failures;

#define CHECK(cond, what)                       \
    do {                                        \
        if (!(cond)) {                          \
            puts("posix: failed: " what);       \
            failures++;                         \
        }                                       \
    } while (0)

int main(int argc, char **argv, char **envp) {
    int fds[2];
    char buf[64];
    ssize_t n;
    pid_t pid;
    pid_t me;
    int status;
    int got;

    puts("posix: main entered");
    CHECK(argc == 2 && argv[1] != 0 && strcmp(argv[1], "from-c") == 0, "argv");
    CHECK(getenv("SLOP_MNU_POSIX") != 0 && strcmp(getenv("SLOP_MNU_POSIX"), "1") == 0, "envp");
    (void)envp;
    me = getpid();
    CHECK(me > 1, "getpid");
    CHECK(getppid() == 1, "getppid");
    CHECK(pipe(fds) == 0, "pipe");

    pid = fork();
    CHECK(pid >= 0, "fork");
    if (pid == 0) {
        /* Child: verify the new pid, then route stdout through the pipe. */
        CHECK(getpid() != me, "child pid");
        close(fds[0]);
        CHECK(dup2(fds[1], 1) == 1, "dup2");
        close(fds[1]);
        if (failures) _exit(8);
        puts("posix: child message 42");
        _exit(failures ? 8 : 7);
    }

    CHECK(pid > 0, "fork parent pid");
    close(fds[1]);
    got = 0;
    for (;;) {
        if (got >= (int)sizeof(buf) - 1) break;
        n = read(fds[0], buf + got, sizeof(buf) - 1 - got);
        if (n <= 0) break;
        got += (int)n;
    }
    buf[got > 0 ? got : 0] = 0;
    CHECK(got > 0, "pipe read");
    CHECK(strcmp(buf, "posix: child message 42\n") == 0, "pipe content");
    CHECK(close(fds[0]) == 0, "close read end");

    CHECK(waitpid(pid, &status, 0) == pid, "waitpid");
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 7, "child status");

    if (failures) {
        printf("posix: %d checks failed\n", failures);
        return 1;
    }
    puts("posix: fork, pipe, dup2 and wait4 passed");
    return 0;
}
