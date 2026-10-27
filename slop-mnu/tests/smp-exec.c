/* A1 exec/fork stress: sequential + parallel fork+execve loops, short-lived
   fork/exit bursts, and AF_UNIX socketpair ping-pong across children.
   Every child is waited; any mismatch prints EXEC_FAIL and exits nonzero.
   Success prints EXEC_OK with exact counts. */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <sys/wait.h>
#include <sys/socket.h>

static char *const hello_argv[] = { "exec-probe", 0 };
static char *const hello_envp[] = { "SLOP_MNU_EXEC=1", 0 };

/* Guest initramfs provides /bin/exec-probe; the Linux native reference points
   SLOB_HELLO at /bin/true (same fork+exec+wait+status mechanism). */
static const char *hello_path(void) {
    const char *p = getenv("SLOB_HELLO");
    return (p && *p) ? p : "/bin/exec-probe";
}

static int run_exec_child(void) {
    pid_t p = fork();
    if (p < 0) { printf("exec: fork failed %d\n", errno); return -1; }
    if (p == 0) {
        execve(hello_path(), hello_argv, hello_envp);
        _exit(127);
    }
    int st = 0;
    if (waitpid(p, &st, 0) < 0) { printf("exec: waitpid failed %d\n", errno); return -1; }
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        printf("exec: bad status pid=%d st=%d\n", (int)p, st); return -1;
    }
    return 0;
}

int main(void) {
    int nexec = 0, nfork = 0, nsock = 0;
    /* Phase 1: 120 sequential execs (AP migration happens underneath). */
    for (int i = 0; i < 120; i++) {
        if (run_exec_child() != 0) { printf("EXEC_FAIL phase1 i=%d\n", i); return 1; }
        nexec++;
    }
    /* Phase 2: 5 rounds x 8 parallel execs. */
    for (int r = 0; r < 5; r++) {
        pid_t ps[8];
        for (int i = 0; i < 8; i++) {
            ps[i] = fork();
            if (ps[i] < 0) { printf("EXEC_FAIL phase2 fork\n"); return 1; }
            if (ps[i] == 0) { execve(hello_path(), hello_argv, hello_envp); _exit(127); }
        }
        for (int i = 0; i < 8; i++) {
            int st = 0;
            if (waitpid(ps[i], &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
                printf("EXEC_FAIL phase2 wait r=%d i=%d st=%d\n", r, i, st); return 1;
            }
            nexec++;
        }
    }
    /* Phase 3: 300 short-lived fork/exit children (status check each). */
    for (int i = 0; i < 300; i++) {
        pid_t p = fork();
        if (p < 0) { printf("EXEC_FAIL phase3 fork\n"); return 1; }
        if (p == 0) _exit(7);
        int st = 0;
        if (waitpid(p, &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 7) {
            printf("EXEC_FAIL phase3 wait i=%d st=%d\n", i, st); return 1;
        }
        nfork++;
    }
    /* Phase 3b: 64 fork/exit/wait in 8-wide parallel groups (32 slot table). */
    for (int g = 0; g < 8; g++) {
        pid_t ps[8];
        for (int i = 0; i < 8; i++) {
            ps[i] = fork();
            if (ps[i] < 0) { printf("EXEC_FAIL phase3b fork\n"); return 1; }
            if (ps[i] == 0) _exit((g * 8 + i) & 0x7f);
        }
        for (int i = 0; i < 8; i++) {
            int st = 0;
            if (waitpid(ps[i], &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != ((g * 8 + i) & 0x7f)) {
                printf("EXEC_FAIL phase3b wait g=%d i=%d st=%d\n", g, i, st); return 1;
            }
            nfork++;
        }
    }
    /* Phase 4: 4 socketpair ping-pong children x 8 rounds (16-slot pool). */
    for (int r = 0; r < 8; r++) {
        int sv[4][2];
        pid_t ps[4];
        for (int i = 0; i < 4; i++) {
            if (socketpair(AF_UNIX, SOCK_STREAM, 0, sv[i]) != 0) {
                printf("EXEC_FAIL socketpair r=%d i=%d errno=%d\n", r, i, errno); return 1;
            }
            ps[i] = fork();
            if (ps[i] < 0) { printf("EXEC_FAIL phase4 fork\n"); return 1; }
            if (ps[i] == 0) {
                close(sv[i][0]);
                char buf[32];
            ssize_t n = read(sv[i][1], buf, sizeof(buf));
            if (n != 5 || memcmp(buf, "ping!", 5) != 0) { printf("EXEC_FAIL phase4 childread n=%d errno=%d\n", (int)n, errno); _exit(21); }
                if (write(sv[i][1], "pong!", 5) != 5) _exit(22);
                _exit(0);
            }
            close(sv[i][1]);
        }
        for (int i = 0; i < 4; i++) {
            ssize_t w = write(sv[i][0], "ping!", 5);
            if (w != 5) { printf("EXEC_FAIL phase4 write r=%d i=%d ret=%d errno=%d\n", r, i, (int)w, errno); return 1; }
            char buf[32];
            ssize_t n = read(sv[i][0], buf, sizeof(buf));
            if (n != 5 || memcmp(buf, "pong!", 5) != 0) {
                printf("EXEC_FAIL phase4 read r=%d i=%d\n", r, i); return 1;
            }
            close(sv[i][0]);
            int st = 0;
            if (waitpid(ps[i], &st, 0) < 0 || !WIFEXITED(st) || WEXITSTATUS(st) != 0) {
                printf("EXEC_FAIL phase4 wait st=%d\n", st); return 1;
            }
            nsock++;
        }
    }
    printf("EXEC_OK exec=%d fork=%d sock=%d\n", nexec, nfork, nsock);
    return 0;
}
